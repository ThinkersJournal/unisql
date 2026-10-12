//! MySQL / MariaDB dialect module: a writer (the [`Mysql`] target). One module, two flavours:
//! MariaDB is MySQL plus `INSERT/DELETE .. RETURNING` (DESIGN.md section 2.1: a child dialect is a
//! table of overrides on its parent).
//!
//! Every deviation from PostgreSQL semantics is a named allowance or a refusal; the differential
//! suite (`unisql-differential`, `tests/postgres_to_mysql.rs`) is what justifies each tier. Tiers
//! assume MySQL 8.4 / MariaDB 11.x with `utf8mb4_bin` as the collation, which is what makes text
//! equality, ordering and `LIKE` agree with a PostgreSQL `C` locale (allowance `text_collation`).

use unisql_core::ast::*;
use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};
use unisql_core::rewrite;
use unisql_core::target::{Ctx, Escape, StmtKind};
use unisql_core::Target;

type R<T> = Result<T, Untranslatable>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    MySql,
    MariaDb,
}

#[derive(Clone, Copy, Debug)]
pub struct Mysql {
    pub flavor: Flavor,
}

impl Mysql {
    pub fn mysql() -> Self {
        Mysql { flavor: Flavor::MySql }
    }
    pub fn mariadb() -> Self {
        Mysql { flavor: Flavor::MariaDb }
    }
}

impl Default for Mysql {
    fn default() -> Self {
        Mysql::mysql()
    }
}

const WINDOW_FNS: &[&str] = &[
    "row_number", "rank", "dense_rank", "percent_rank", "cume_dist", "ntile", "lag", "lead",
    "first_value", "last_value", "nth_value",
];

impl Target for Mysql {
    fn name(&self) -> &'static str {
        match self.flavor {
            Flavor::MySql => "mysql",
            Flavor::MariaDb => "mariadb",
        }
    }

    fn quote_ident(&self, s: &str) -> String {
        format!("`{}`", s.replace('`', "``"))
    }

    fn string_literal(&self, s: &str) -> String {
        // Backslash is an escape character in MySQL string literals (unlike PostgreSQL).
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
    }

    fn placeholder(&self, _n: u32) -> String {
        // Positional: the caller binds in `Translation::param_order`.
        "?".to_string()
    }

    fn bool_literal(&self, b: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "boolean_literal",
            Tier::Conditional,
            "MySQL BOOLEAN is TINYINT(1); results come back as integers",
            Some(Allowance::BooleanAsInteger),
        )?;
        Ok(if b { "TRUE" } else { "FALSE" }.to_string())
    }

    fn like(
        &self,
        lhs: &str,
        rhs: &str,
        negated: bool,
        ci: bool,
        esc: Escape<'_>,
        ctx: &mut Ctx,
    ) -> R<String> {
        let not = if negated { "NOT " } else { "" };
        let tail = match esc {
            // MySQL's default escape is also a backslash.
            Escape::Default => String::new(),
            Escape::None => " ESCAPE ''".to_string(),
            Escape::Custom(c) => format!(" ESCAPE {c}"),
        };
        ctx.note(
            if ci { "ilike" } else { "like" },
            Tier::Conditional,
            "LIKE case sensitivity follows the column collation: exact only under a binary (_bin) collation",
            Some(Allowance::LikeCaseSensitivityIsConnectionSetting),
        )?;
        if ci {
            Ok(format!("LOWER({lhs}) {not}LIKE LOWER({rhs}){tail}"))
        } else {
            Ok(format!("{lhs} {not}LIKE {rhs}{tail}"))
        }
    }

    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        let n = f.name.as_str();
        let call = |name: &str, a: &[String]| format!("{}({})", name.to_uppercase(), a.join(", "));
        if f.over.is_some() && WINDOW_FNS.contains(&n) {
            ctx.note(format!("fn:{n}"), Tier::Exact, "window function (MySQL >= 8.0)", None)?;
            return Ok(call(n, args));
        }
        match n {
            "count" => {
                ctx.note("fn:count", Tier::Exact, "", None)?;
                Ok(if f.star {
                    "COUNT(*)".to_string()
                } else {
                    format!("COUNT({}{})", if f.distinct { "DISTINCT " } else { "" }, args.join(", "))
                })
            }
            "sum" | "min" | "max" | "avg" => {
                ctx.note(
                    format!("fn:{n}"),
                    if matches!(n, "min" | "max") { Tier::Conditional } else { Tier::Exact },
                    "on text, follows the column collation",
                    if matches!(n, "min" | "max") { Some(Allowance::TextCollation) } else { None },
                )?;
                Ok(format!("{}({}{})", n.to_uppercase(), if f.distinct { "DISTINCT " } else { "" }, args.join(", ")))
            }
            "coalesce" | "nullif" | "abs" | "replace" | "ltrim" | "rtrim" | "md5" => {
                ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
                Ok(call(n, args))
            }
            "lower" | "upper" => {
                ctx.note(
                    format!("fn:{n}"),
                    Tier::Conditional,
                    "case folding outside ASCII differs between the engines and their locales",
                    Some(Allowance::AsciiOnlyCaseFolding),
                )?;
                Ok(call(n, args))
            }
            // MySQL LENGTH() is bytes; PostgreSQL length() is characters.
            "length" | "char_length" | "character_length" => {
                ctx.note(format!("fn:{n}"), Tier::Emulated, "-> CHAR_LENGTH (MySQL LENGTH counts bytes)", None)?;
                Ok(call("char_length", args))
            }
            "octet_length" => {
                ctx.note("fn:octet_length", Tier::Emulated, "-> LENGTH (bytes)", None)?;
                Ok(call("length", args))
            }
            "btrim" => {
                ctx.note("fn:btrim", Tier::Emulated, "-> TRIM", None)?;
                Ok(call("trim", args))
            }
            "string_agg" => {
                if args.len() != 2 {
                    return Err(ctx.refuse("fn:string_agg", "string_agg needs (value, separator)", &[]));
                }
                ctx.note(
                    "fn:string_agg",
                    Tier::Conditional,
                    "-> GROUP_CONCAT(x SEPARATOR s); the result is silently truncated at group_concat_max_len",
                    Some(Allowance::AggregateStringTruncation),
                )?;
                let order = ctx.agg_order.take().map(|o| format!(" ORDER BY {o}")).unwrap_or_default();
                Ok(format!(
                    "GROUP_CONCAT({}{}{} SEPARATOR {})",
                    if f.distinct { "DISTINCT " } else { "" },
                    args[0],
                    order,
                    args[1]
                ))
            }
            "now" => {
                ctx.note(
                    "fn:now",
                    Tier::Conditional,
                    "-> NOW(): a DATETIME without time-zone semantics, in the session time zone",
                    Some(Allowance::NowAsText),
                )?;
                Ok("NOW()".to_string())
            }
            "substr" | "substring" => Err(ctx.refuse(
                &format!("fn:{n}"),
                "start positions before 1 behave differently (PostgreSQL clips the length, MySQL returns '')",
                &["use a start position >= 1"],
            )),
            "concat" => Err(ctx.refuse(
                "fn:concat",
                "PostgreSQL concat() ignores NULLs; MySQL CONCAT() returns NULL if any argument is NULL",
                &["use the || operator, which is translated"],
            )),
            "make_interval" => Err(ctx.refuse(
                "fn:make_interval",
                "only `timestamp +/- make_interval(secs|mins|hours|days|weeks => ..)` is translated",
                &[],
            )),
            other => Err(ctx.refuse(
                &format!("fn:{other}"),
                "no verified mapping for this function on MySQL/MariaDB yet",
                &["add a mapping with a differential test"],
            )),
        }
    }

    fn cast(&self, expr: &str, ty: &SqlType, ctx: &mut Ctx) -> R<String> {
        let name = match ty {
            SqlType::SmallInt | SqlType::Integer | SqlType::BigInt => "SIGNED".to_string(),
            SqlType::Real | SqlType::Double => "DOUBLE".to_string(),
            SqlType::Numeric { precision: Some(p), scale } => format!("DECIMAL({p}, {})", scale.unwrap_or(0)),
            SqlType::Text | SqlType::Varchar(_) => "CHAR".to_string(),
            SqlType::Date => "DATE".to_string(),
            SqlType::Timestamp { with_tz: false } => "DATETIME".to_string(),
            other => {
                return Err(ctx.refuse(
                    "cast",
                    &format!("CAST to {other:?} has no MySQL equivalent that preserves meaning"),
                    &["store the value in a column of the right type and convert in the application"],
                ));
            }
        };
        ctx.note(
            "cast",
            Tier::Conditional,
            "MySQL casts leniently: a malformed value becomes 0 or a prefix (with a warning) instead of an error",
            Some(Allowance::LenientCast),
        )?;
        Ok(format!("CAST({expr} AS {name})"))
    }

    fn rewrite_query(&self, q: &Query, ctx: &mut Ctx) -> R<Option<Query>> {
        if let SetExpr::Select(s) = &q.body {
            if matches!(s.distinct, Distinct::On(_)) {
                return match rewrite::distinct_on_via_window(q) {
                    Ok(q2) => {
                        ctx.note(
                            "distinct_on",
                            Tier::Emulated,
                            "row_number() OVER (PARTITION BY keys ORDER BY ..) = 1 in a derived table",
                            None,
                        )?;
                        Ok(Some(q2))
                    }
                    Err(why) => Err(ctx.refuse("distinct_on", &why, &["list the columns explicitly"])),
                };
            }
        }
        Ok(None)
    }

    fn feature(&self, name: &'static str, ctx: &mut Ctx) -> R<()> {
        match name {
            "window_function" => ctx.note(name, Tier::Exact, "requires MySQL >= 8.0 / MariaDB >= 10.2", None),
            "recursive_cte" => ctx.note(name, Tier::Exact, "requires MySQL >= 8.0 / MariaDB >= 10.2", None),
            "filter_clause" => Err(ctx.refuse(
                "filter_clause",
                "MySQL has no FILTER (WHERE ..) on aggregates",
                &["use SUM(CASE WHEN .. THEN 1 ELSE 0 END)"],
            )),
            "named_arg" => Err(ctx.refuse("named_arg", "MySQL functions have no named-argument notation", &[])),
            _ => ctx.note(name, Tier::Exact, "", None),
        }
    }

    fn trim(&self, side: TrimSide, expr: &str, chars: Option<&str>, ctx: &mut Ctx) -> R<String> {
        // PostgreSQL's trim characters are a SET; MySQL's remstr is a STRING removed repeatedly.
        // They agree only for a single character.
        if let Some(c) = chars {
            let single = c.len() == 3 && c.starts_with('\'') && c.ends_with('\'');
            if !single {
                return Err(ctx.refuse(
                    "trim",
                    "PostgreSQL trims a set of characters, MySQL a repeated string: they agree only for one character",
                    &["use a single-character literal"],
                ));
            }
        }
        ctx.note("trim", Tier::Exact, "", None)?;
        let kw = match side {
            TrimSide::Both => "BOTH",
            TrimSide::Leading => "LEADING",
            TrimSide::Trailing => "TRAILING",
        };
        Ok(match chars {
            Some(c) => format!("TRIM({kw} {c} FROM {expr})"),
            None => format!("TRIM({kw} FROM {expr})"),
        })
    }

    fn interval_arith(
        &self,
        minus: bool,
        left: &str,
        parts: &[(String, String)],
        ctx: &mut Ctx,
    ) -> R<Option<String>> {
        let mut out = left.to_string();
        for (unit, value) in parts {
            let (u, mult) = match unit.as_str() {
                "secs" => ("SECOND", None),
                "mins" => ("MINUTE", None),
                "hours" => ("HOUR", None),
                "days" => ("DAY", None),
                "weeks" => ("DAY", Some(7)),
                other => {
                    return Err(ctx.refuse(
                        "fn:make_interval",
                        &format!("make_interval({other} => ..): month/year arithmetic differs between the engines"),
                        &[],
                    ));
                }
            };
            let v = match mult {
                Some(m) => format!("({value}) * {m}"),
                None => value.clone(),
            };
            out = format!("({out} {} INTERVAL ({v}) {u})", if minus { "-" } else { "+" });
        }
        ctx.note(
            "interval_arithmetic",
            Tier::Conditional,
            "timestamp +/- make_interval(..) becomes DATETIME +/- INTERVAL: no time-zone semantics",
            Some(Allowance::TimestampArithmeticAsText),
        )?;
        Ok(Some(out))
    }

    fn json_access(&self, text: bool, _lhs: &str, _rhs: &str, ctx: &mut Ctx) -> R<String> {
        Err(ctx.refuse(
            if text { "json_get_text" } else { "json_get" },
            "MySQL JSON access uses path expressions ('$.key'); translating a key to a path is not implemented",
            &[],
        ))
    }

    fn distinct_on(&self, ctx: &mut Ctx) -> R<()> {
        // Reached only if a DISTINCT ON survived rewrite_query, which it cannot.
        Err(ctx.refuse("distinct_on", "unexpected DISTINCT ON", &[]))
    }

    fn returning(&self, kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        match (self.flavor, kind) {
            (Flavor::MariaDb, StmtKind::Update) => Err(ctx.refuse(
                "returning",
                "MariaDB supports RETURNING on INSERT and DELETE only, not on UPDATE",
                &["run the UPDATE, then SELECT the rows back in the same transaction"],
            )),
            (Flavor::MariaDb, _) => ctx.note(
                "returning",
                Tier::Exact,
                "MariaDB >= 10.5 for INSERT and DELETE",
                None,
            ),
            (Flavor::MySql, _) => Err(ctx.refuse(
                "returning",
                "MySQL has no RETURNING clause",
                &["select the row back with a second statement", "use MariaDB >= 10.5 for INSERT/DELETE"],
            )),
        }
    }

    fn upsert_clause(
        &self,
        _conflict_cols: &[String],
        do_nothing: bool,
        set: &str,
        where_: Option<&str>,
        first_insert_col: &str,
        ctx: &mut Ctx,
    ) -> R<String> {
        if where_.is_some() {
            return Err(ctx.refuse(
                "upsert_where",
                "ON DUPLICATE KEY UPDATE has no WHERE clause",
                &["move the condition into the SET expressions with IF()/CASE"],
            ));
        }
        ctx.note(
            "upsert",
            Tier::Conditional,
            "ON DUPLICATE KEY UPDATE fires on ANY unique key, not the stated conflict target",
            Some(Allowance::UpsertAnyUniqueKey),
        )?;
        if do_nothing {
            if first_insert_col.is_empty() {
                return Err(ctx.refuse("upsert", "DO NOTHING needs at least one inserted column", &[]));
            }
            Ok(format!(" ON DUPLICATE KEY UPDATE {first_insert_col} = {first_insert_col}"))
        } else {
            Ok(format!(" ON DUPLICATE KEY UPDATE {set}"))
        }
    }

    fn excluded_ref(&self, column_quoted: &str) -> String {
        format!("VALUES({column_quoted})")
    }

    fn limit_offset(&self, limit: Option<&str>, offset: Option<&str>, ctx: &mut Ctx) -> R<String> {
        match (limit, offset) {
            (None, None) => Ok(String::new()),
            (Some(l), None) => {
                ctx.note("limit", Tier::Exact, "", None)?;
                Ok(format!(" LIMIT {l}"))
            }
            (Some(l), Some(o)) => {
                ctx.note("limit", Tier::Exact, "", None)?;
                ctx.note("offset", Tier::Exact, "", None)?;
                Ok(format!(" LIMIT {l} OFFSET {o}"))
            }
            (None, Some(o)) => {
                ctx.note("offset", Tier::Emulated, "MySQL requires LIMIT with OFFSET; 2^64-1 means unbounded", None)?;
                Ok(format!(" LIMIT 18446744073709551615 OFFSET {o}"))
            }
        }
    }

    fn order_item(&self, expr: &str, desc: bool, nulls_first: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "order_by",
            Tier::Conditional,
            "text ordering follows the column collation",
            Some(Allowance::TextCollation),
        )?;
        let dir = if desc { " DESC" } else { "" };
        // MySQL: NULL is the smallest value (first ascending, last descending).
        if nulls_first == !desc {
            ctx.note("null_ordering", Tier::Exact, "", None)?;
            return Ok(format!("{expr}{dir}"));
        }
        if expr.contains('?') {
            // The expression is printed twice below; a positional `?` would be bound once.
            return Err(ctx.refuse(
                "null_ordering",
                "NULL placement is emulated by repeating the sort expression, which contains a parameter",
                &["write ORDER BY (x IS NULL), x explicitly"],
            ));
        }
        ctx.note(
            "null_ordering",
            Tier::Emulated,
            "NULLS FIRST/LAST emulated with a leading (expr IS NULL) sort key",
            None,
        )?;
        let lead = if nulls_first { " DESC" } else { "" };
        Ok(format!("({expr} IS NULL){lead}, {expr}{dir}"))
    }

    fn binary_override(&self, op: BinaryOp, l: &str, r: &str, numeric: bool, ctx: &mut Ctx) -> R<Option<String>> {
        match op {
            BinaryOp::Concat => {
                ctx.note("concat", Tier::Exact, "|| is OR in MySQL, so CONCAT() is used", None)?;
                Ok(Some(format!("CONCAT({l}, {r})")))
            }
            BinaryOp::Divide => {
                let _ = numeric;
                ctx.note(
                    "division_integer",
                    Tier::Conditional,
                    "/ yields a DECIMAL in MySQL; DIV is integer division, valid only if both operands are integers",
                    Some(Allowance::IntegerDivisionAssumed),
                )?;
                Ok(Some(format!("{l} DIV {r}")))
            }
            _ => Ok(None),
        }
    }

    fn binary_op(&self, op: BinaryOp, numeric: bool, ctx: &mut Ctx) -> R<()> {
        match op {
            BinaryOp::Divide | BinaryOp::Modulo => ctx.note(
                "division",
                Tier::Conditional,
                "division/modulo by zero is NULL (with a warning) on MySQL and an error on PostgreSQL",
                Some(Allowance::DivisionByZeroIsNull),
            ),
            BinaryOp::Eq | BinaryOp::NotEq | BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
                if !numeric =>
            {
                ctx.note(
                    "text_comparison",
                    Tier::Conditional,
                    "text comparison follows the column collation (case-insensitive by default in MySQL)",
                    Some(Allowance::TextCollation),
                )
            }
            _ => Ok(()),
        }
    }

    fn array_membership(&self, lhs: &str, ph: &str, negated: bool, n: u32, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "array_param",
            Tier::Conditional,
            "bind the array as JSON text; membership via JSON_CONTAINS(arr, JSON_ARRAY(x)). NULLs are definite",
            Some(Allowance::ArrayNullSemantics),
        )?;
        ctx.bind_array_param(n, ParamBinding::JsonArrayText);
        let not = if negated { "NOT " } else { "" };
        Ok(format!("{not}JSON_CONTAINS({ph}, JSON_ARRAY({lhs}))"))
    }

    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {
        if numeric {
            return Ok(());
        }
        ctx.note(
            "text_comparison",
            Tier::Conditional,
            "text comparison follows the column collation",
            Some(Allowance::TextCollation),
        )
    }

    fn update_from_tier(&self) -> Tier {
        Tier::Refused
    }

    fn nulls_order(&self, _desc: bool, _nulls_first: bool, _ctx: &mut Ctx) -> R<String> {
        Ok(String::new()) // handled in order_item
    }
}
