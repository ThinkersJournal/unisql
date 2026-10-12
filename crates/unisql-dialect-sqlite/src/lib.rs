//! SQLite dialect module: a reader (text -> IR) and a writer (the [`Sqlite`] target).
//!
//! Every deviation from PostgreSQL semantics is a named [`Allowance`] or a refusal; the
//! differential tests in `unisql-differential` are what justify each tier. Minimum engine
//! version assumed by the tiers: SQLite 3.35 (`RETURNING`), noted per construct.

use unisql_core::ast::*;
use unisql_core::rewrite;
use unisql_core::report::{Allowance, Tier, Untranslatable};
use unisql_core::target::{generic_call, generic_call_ordered, Ctx, Escape, StmtKind};
use unisql_core::{Read, ReadError, Target};

/// Read exactly one SQLite statement.
pub fn read(sql: &str) -> Result<Read, ReadError> {
    unisql_sqlparser_adapter::read(sql, &unisql_sqlparser_adapter::SQLITE)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Sqlite;

type R<T> = Result<T, Untranslatable>;

impl Target for Sqlite {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn placeholder(&self, n: u32) -> String {
        // Numbered, so a source `$2, $1` or a reused `$1` binds without reordering.
        format!("?{n}")
    }

    fn bool_literal(&self, b: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "boolean_literal",
            Tier::Conditional,
            "SQLite has no boolean type; TRUE/FALSE are 1/0 and results come back as integers",
            Some(Allowance::BooleanAsInteger),
        )?;
        Ok(if b { "1" } else { "0" }.to_string())
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
        // SQLite has no default escape character, so the IR's choice is always stated explicitly.
        let tail = match esc {
            Escape::None => String::new(),
            Escape::Default => " ESCAPE '\\'".to_string(),
            Escape::Custom(c) => format!(" ESCAPE {c}"),
        };
        if ci {
            // PostgreSQL's default LIKE escape is a backslash; SQLite has none, so state it.
            ctx.note(
                "ilike",
                Tier::Conditional,
                "emulated as lower(a) LIKE lower(b) ESCAPE; lower() folds ASCII only",
                Some(Allowance::AsciiOnlyCaseFolding),
            )?;
            ctx.note(
                "ilike",
                Tier::Conditional,
                "the target's LIKE case sensitivity is a connection setting (PRAGMA case_sensitive_like)",
                Some(Allowance::LikeCaseSensitivityIsConnectionSetting),
            )?;
            Ok(format!("lower({lhs}) {not}LIKE lower({rhs}){tail}"))
        } else {
            ctx.note(
                "like",
                Tier::Conditional,
                "SQLite LIKE is ASCII case-insensitive unless PRAGMA case_sensitive_like=ON; \
                 an ESCAPE clause is added to match PostgreSQL's default escape character",
                Some(Allowance::LikeCaseSensitivityIsConnectionSetting),
            )?;
            Ok(format!("{lhs} {not}LIKE {rhs}{tail}"))
        }
    }

    fn rewrite_query(&self, q: &Query, ctx: &mut Ctx) -> R<Option<Query>> {
        if let SetExpr::Select(s) = &q.body {
            if matches!(s.distinct, Distinct::On(_)) {
                return match rewrite::distinct_on_via_window(q) {
                    Ok(q2) => {
                        ctx.note(
                            "distinct_on",
                            Tier::Emulated,
                            "row_number() OVER (PARTITION BY keys ORDER BY ..) = 1 in a derived table; \
                             without ORDER BY the surviving row per key is arbitrary on both engines",
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
        let min = match name {
            "window_function" => "requires SQLite >= 3.25",
            "filter_clause" => "requires SQLite >= 3.30",
            "recursive_cte" => "requires SQLite >= 3.8.3",
            "named_arg" => {
                return Err(ctx.refuse(
                    "named_arg",
                    "SQLite functions have no named-argument notation",
                    &["use positional arguments"],
                ));
            }
            _ => "",
        };
        ctx.note(name, Tier::Exact, min, None)
    }

    fn trim(&self, side: TrimSide, expr: &str, chars: Option<&str>, ctx: &mut Ctx) -> R<String> {
        ctx.note("trim", Tier::Exact, "", None)?;
        let f = match side {
            TrimSide::Both => "trim",
            TrimSide::Leading => "ltrim",
            TrimSide::Trailing => "rtrim",
        };
        Ok(match chars {
            Some(c) => format!("{f}({expr}, {c})"),
            None => format!("{f}({expr})"),
        })
    }

    fn interval_arith(
        &self,
        minus: bool,
        left: &str,
        parts: &[(String, String)],
        ctx: &mut Ctx,
    ) -> R<Option<String>> {
        let mut mods = Vec::new();
        for (unit, value) in parts {
            let (sql_unit, mult) = match unit.as_str() {
                "secs" => ("seconds", None),
                "mins" => ("minutes", None),
                "hours" => ("hours", None),
                "days" => ("days", None),
                "weeks" => ("days", Some(7)),
                other => {
                    return Err(ctx.refuse(
                        "fn:make_interval",
                        &format!("make_interval({other} => ..): month/year arithmetic differs between the engines"),
                        &["use secs/mins/hours/days/weeks"],
                    ));
                }
            };
            let v = match mult {
                Some(m) => format!("(({value}) * {m})"),
                None => format!("({value})"),
            };
            let signed = if minus { format!("(-{v})") } else { v };
            mods.push(format!("printf('%+f {sql_unit}', {signed})"));
        }
        ctx.note(
            "interval_arithmetic",
            Tier::Conditional,
            "timestamp +/- make_interval(..) becomes datetime(ts, modifiers): text, whole seconds, UTC",
            Some(Allowance::TimestampArithmeticAsText),
        )?;
        Ok(Some(format!("datetime({left}, {})", mods.join(", "))))
    }

    fn json_access(&self, text: bool, lhs: &str, rhs: &str, ctx: &mut Ctx) -> R<String> {
        if text {
            ctx.note(
                "json_get_text",
                Tier::Conditional,
                "SQLite ->> keeps the scalar's type (true -> 1); cast to TEXT here, but a JSON boolean \
                 still reads '1' where PostgreSQL reads 'true'. Requires SQLite >= 3.38",
                Some(Allowance::JsonScalarAffinity),
            )?;
            Ok(format!("CAST({lhs} ->> {rhs} AS TEXT)"))
        } else {
            ctx.note(
                "json_get",
                Tier::Conditional,
                "SQLite -> returns JSON text; comparison is textual, not jsonb equality. Requires >= 3.38",
                Some(Allowance::JsonRepresentationAsText),
            )?;
            Ok(format!("{lhs} -> {rhs}"))
        }
    }

    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        let n = f.name.as_str();
        if ctx.source == "sqlite" {
            if n == "now" {
                // The reader normalised CURRENT_TIMESTAMP to now(); SQLite spells it back.
                ctx.note("fn:now", Tier::Exact, "", None)?;
                return Ok("CURRENT_TIMESTAMP".to_string());
            }
            // Same dialect in and out: the reader accepted the name, so this is the identity.
            ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
            let order = ctx.agg_order.take();
            return Ok(generic_call_ordered(n, args, f.distinct, f.star, order.as_deref()));
        }
        if f.over.is_some()
            && matches!(
                n,
                "row_number" | "rank" | "dense_rank" | "percent_rank" | "cume_dist" | "ntile" | "lag"
                    | "lead" | "first_value" | "last_value" | "nth_value"
            )
        {
            ctx.note(format!("fn:{n}"), Tier::Exact, "window function", None)?;
            return Ok(generic_call(n, args, false, false));
        }
        match n {
            "md5" | "sha256" | "sha1" | "sha512" | "encode" | "decode" | "convert_to" | "convert_from" => {
                return Err(ctx.refuse(
                    &format!("fn:{n}"),
                    "SQLite has no built-in hashing/encoding functions; the result would depend on a \
                     host-registered function or extension, which UniSQL cannot assume",
                    &["hash in the application", "register the function and use a raw(sqlite, ..) fragment"],
                ));
            }
            "current_schema" | "current_database" | "current_user" | "session_user" | "to_regclass"
            | "pg_backend_pid" | "set_config" | "current_setting" => {
                return Err(ctx.refuse(
                    &format!("fn:{n}"),
                    "PostgreSQL catalog/session introspection has no SQLite equivalent",
                    &["query sqlite_schema (a different, SQLite-specific statement)"],
                ));
            }
            "make_interval" => {
                return Err(ctx.refuse(
                    "fn:make_interval",
                    "SQLite has no interval type; only `timestamp +/- make_interval(secs|mins|hours|days|weeks => ..)` is translated",
                    &[],
                ));
            }
            "regexp_replace" | "regexp_match" | "regexp_matches" | "split_part" => {
                return Err(ctx.refuse(
                    &format!("fn:{n}"),
                    "no built-in regular-expression or split function in SQLite",
                    &[],
                ));
            }
            _ => {}
        }
        match n {
            "min" | "max" => {
                ctx.note(
                    format!("fn:{n}"),
                    Tier::Conditional,
                    "on text, follows the target's collation (BINARY by default)",
                    Some(Allowance::TextCollation),
                )?;
                Ok(generic_call(n, args, f.distinct, f.star))
            }
            "count" | "sum" | "avg" | "coalesce" | "nullif" | "abs" | "length" | "replace" | "trim"
            | "ltrim" | "rtrim" | "substr" | "ifnull" => {
                ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
                Ok(generic_call(n, args, f.distinct, f.star))
            }
            "lower" | "upper" => {
                ctx.note(
                    format!("fn:{n}"),
                    Tier::Conditional,
                    "SQLite folds ASCII only",
                    Some(Allowance::AsciiOnlyCaseFolding),
                )?;
                Ok(generic_call(n, args, false, false))
            }
            "btrim" => {
                ctx.note("fn:btrim", Tier::Emulated, "-> trim(x[, chars])", None)?;
                Ok(generic_call("trim", args, false, false))
            }
            "octet_length" => {
                if args.len() != 1 {
                    return Err(ctx.refuse("fn:octet_length", "octet_length takes one argument", &[]));
                }
                ctx.note(
                    "fn:octet_length",
                    Tier::Emulated,
                    "-> length(CAST(x AS BLOB)): bytes, not characters",
                    None,
                )?;
                Ok(format!("LENGTH(CAST({} AS BLOB))", args[0]))
            }
            "char_length" | "character_length" => {
                ctx.note(format!("fn:{n}"), Tier::Emulated, "-> length()", None)?;
                Ok(generic_call("length", args, false, false))
            }
            "string_agg" => {
                if args.len() != 2 {
                    return Err(ctx.refuse(
                        "fn:string_agg",
                        "string_agg needs exactly (value, separator)",
                        &[],
                    ));
                }
                ctx.note(
                    "fn:string_agg",
                    Tier::Emulated,
                    "-> group_concat(value, separator); an ORDER BY inside needs SQLite >= 3.44",
                    None,
                )?;
                let order = ctx.agg_order.take();
                Ok(generic_call_ordered("group_concat", args, f.distinct, false, order.as_deref()))
            }
            "now" => {
                ctx.note(
                    "fn:now",
                    Tier::Conditional,
                    "-> CURRENT_TIMESTAMP; text 'YYYY-MM-DD HH:MM:SS' in UTC, not a timestamptz",
                    Some(Allowance::NowAsText),
                )?;
                Ok("CURRENT_TIMESTAMP".to_string())
            }
            other => Err(ctx.refuse(
                &format!("fn:{other}"),
                "no verified mapping for this function on SQLite yet",
                &["add a mapping with a differential test", "use a raw(sqlite, ..) fragment"],
            )),
        }
    }

    fn cast(&self, expr: &str, ty: &SqlType, ctx: &mut Ctx) -> R<String> {
        let name = match ty {
            SqlType::SmallInt | SqlType::Integer | SqlType::BigInt => "INTEGER",
            SqlType::Real | SqlType::Double => "REAL",
            SqlType::Numeric { .. } => "NUMERIC",
            SqlType::Text | SqlType::Varchar(_) => "TEXT",
            other => {
                return Err(ctx.refuse(
                    "cast",
                    &format!("CAST to {other:?} has no SQLite equivalent that preserves meaning"),
                    &["store as TEXT/INTEGER and convert in the application"],
                ));
            }
        };
        ctx.note(
            "cast",
            Tier::Conditional,
            "SQLite casts leniently: a malformed value becomes 0 or a numeric prefix instead of an error",
            Some(Allowance::LenientCast),
        )?;
        Ok(format!("CAST({expr} AS {name})"))
    }

    fn distinct_on(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "distinct_on",
            "SQLite has no DISTINCT ON",
            &["rewrite with row_number() OVER (PARTITION BY ..) in a subquery (not yet emulated)"],
        ))
    }

    fn row_lock(&self, _lock: RowLock, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "row_lock",
            "SQLite has no row-level locks (it locks the whole database)",
            &["use BEGIN IMMEDIATE for a write transaction"],
        ))
    }

    fn returning(&self, _kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "requires SQLite >= 3.35", None)
    }

    fn upsert_clause(
        &self,
        conflict_cols: &[String],
        do_nothing: bool,
        set: &str,
        where_: Option<&str>,
        _first_insert_col: &str,
        ctx: &mut Ctx,
    ) -> R<String> {
        ctx.note(
            if where_.is_some() { "upsert_where" } else { "upsert" },
            Tier::Exact,
            "requires SQLite >= 3.24",
            None,
        )?;
        let mut s = String::from(" ON CONFLICT");
        if !conflict_cols.is_empty() {
            s.push_str(&format!(" ({})", conflict_cols.join(", ")));
        }
        if do_nothing {
            s.push_str(" DO NOTHING");
        } else {
            s.push_str(&format!(" DO UPDATE SET {set}"));
            if let Some(w) = where_ {
                s.push_str(&format!(" WHERE {w}"));
            }
        }
        Ok(s)
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
                ctx.note(
                    "offset",
                    Tier::Emulated,
                    "SQLite requires LIMIT with OFFSET; -1 means unlimited",
                    None,
                )?;
                Ok(format!(" LIMIT -1 OFFSET {o}"))
            }
        }
    }

    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {
        if numeric {
            return ctx.note("ordered_comparison", Tier::Exact, "numeric operand", None);
        }
        ctx.note(
            "ordered_comparison",
            Tier::Conditional,
            "text comparison follows the target's collation (BINARY by default)",
            Some(Allowance::TextCollation),
        )
    }

    fn binary_op(&self, op: BinaryOp, numeric: bool, ctx: &mut Ctx) -> R<()> {
        match op {
            BinaryOp::Divide | BinaryOp::Modulo => ctx.note(
                "division",
                Tier::Conditional,
                "division/modulo by zero is NULL on SQLite and an error on PostgreSQL",
                Some(Allowance::DivisionByZeroIsNull),
            ),
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq if numeric => {
                ctx.note("ordered_comparison", Tier::Exact, "numeric operand", None)
            }
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => ctx.note(
                "ordered_comparison",
                Tier::Conditional,
                "text comparison follows the target's collation (BINARY by default)",
                Some(Allowance::TextCollation),
            ),
            _ => Ok(()),
        }
    }

    fn nulls_order(&self, desc: bool, nulls_first: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "order_by",
            Tier::Conditional,
            "text ordering follows the target's collation (BINARY by default)",
            Some(Allowance::TextCollation),
        )?;
        // SQLite default: NULLs sort as the smallest value (first ascending, last descending).
        let default_first = !desc;
        if nulls_first == default_first {
            ctx.note("null_ordering", Tier::Exact, "", None)?;
            Ok(String::new())
        } else {
            ctx.note(
                "null_ordering",
                Tier::Emulated,
                "explicit NULLS FIRST/LAST (SQLite >= 3.30) to keep PostgreSQL's NULL order",
                None,
            )?;
            Ok(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" }.to_string())
        }
    }
}
