// SPDX-License-Identifier: MIT OR Apache-2.0
//! MySQL / MariaDB dialect module: a writer (the [`Mysql`] target). One module, two flavours:
//! MariaDB is MySQL plus `INSERT/DELETE .. RETURNING` (DESIGN.md section 2.1: a child dialect is a
//! table of overrides on its parent).
//!
//! Every deviation from PostgreSQL semantics is a named allowance or a refusal; the differential
//! suite (`unisql-differential`, `tests/postgres_to_mysql.rs`) is what justifies each tier. Tiers
//! assume MySQL 8.4 / MariaDB 11.x with `utf8mb4_bin` as the collation, which is what makes text
//! equality, ordering and `LIKE` agree with a PostgreSQL `C` locale (allowance `text_collation`).

use unisql_core::Target;
use unisql_core::ast::*;
use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};
use unisql_core::rewrite;
use unisql_core::target::{Ctx, DmlLimit, DmlSources, Escape, StmtKind};

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
    pub fn for_mysql() -> Self {
        Mysql {
            flavor: Flavor::MySql,
        }
    }
    pub fn for_mariadb() -> Self {
        Mysql {
            flavor: Flavor::MariaDb,
        }
    }

    /// Read exactly one MySQL / MariaDB statement.
    pub fn read(sql: &str) -> Result<unisql_core::Read, unisql_core::ReadError> {
        unisql_sqlparser_adapter::read(sql, &unisql_sqlparser_adapter::MYSQL)
    }
}

impl Default for Mysql {
    fn default() -> Self {
        Mysql::for_mysql()
    }
}

const WINDOW_FNS: &[&str] = &[
    "row_number",
    "rank",
    "dense_rank",
    "percent_rank",
    "cume_dist",
    "ntile",
    "lag",
    "lead",
    "first_value",
    "last_value",
    "nth_value",
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

    fn bytes_literal(&self, b: &[u8]) -> String {
        let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
        format!("X'{hex}'")
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
        // On MySQL/MariaDB LIKE follows the column collation (not a connection setting, as on
        // SQLite): exact under a binary collation, which a gate can read from the configuration.
        ctx.note(
            if ci { "ilike" } else { "like" },
            Tier::Conditional,
            "LIKE case sensitivity follows the column collation: exact only under a binary (_bin) collation",
            Some(Allowance::TextCollation),
        )?;
        if ci {
            // ILIKE is spelled LOWER(a) LIKE LOWER(b): MySQL lowercases beyond ASCII, PostgreSQL's
            // ILIKE in the C locale does not.
            ctx.note(
                "ilike",
                Tier::Conditional,
                "case folding outside ASCII differs between the engines and their locales",
                Some(Allowance::AsciiOnlyCaseFolding),
            )?;
            Ok(format!("LOWER({lhs}) {not}LIKE LOWER({rhs}){tail}"))
        } else {
            Ok(format!("{lhs} {not}LIKE {rhs}{tail}"))
        }
    }

    fn regexp(&self, lhs: &str, rhs: &str, negated: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "regexp",
            Tier::Conditional,
            "regular-expression syntax and case sensitivity differ between engines; exact only for \
             patterns in the common subset (MySQL matching also follows the column collation)",
            Some(Allowance::RegexpFlavor),
        )?;
        Ok(format!(
            "{lhs} {}REGEXP {rhs}",
            if negated { "NOT " } else { "" }
        ))
    }

    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        let n = f.name.as_str();
        let call = |name: &str, a: &[String]| format!("{}({})", name.to_uppercase(), a.join(", "));
        if f.over.is_some() && WINDOW_FNS.contains(&n) {
            ctx.note(
                format!("fn:{n}"),
                Tier::Exact,
                "window function (MySQL >= 8.0)",
                None,
            )?;
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
            // PostgreSQL's random (version 4) UUID, built from the CSPRNG: version nibble 4, variant
            // nibble 8/9/a/b chosen uniformly. Verified: 2000 values per engine all match the v4 shape
            // and are distinct (the differential suite repeats the check on every run).
            "gen_random_uuid" => {
                ctx.note(
                    "fn:gen_random_uuid",
                    Tier::Emulated,
                    "assembled from RANDOM_BYTES: version 4, variant 10xx",
                    None,
                )?;
                Ok("LOWER(CONCAT(HEX(RANDOM_BYTES(4)), '-', HEX(RANDOM_BYTES(2)), '-4', \
                    SUBSTR(HEX(RANDOM_BYTES(2)), 2), '-', \
                    ELT(1 + (ORD(RANDOM_BYTES(1)) & 3), '8', '9', 'a', 'b'), \
                    SUBSTR(HEX(RANDOM_BYTES(2)), 2), '-', HEX(RANDOM_BYTES(6))))"
                    .to_string())
            }
            // to_timestamp(seconds): FROM_UNIXTIME, a DATETIME in the session zone (UTC here); the
            // fraction of a fractional argument is kept. The two-argument (text, format) form is
            // PostgreSQL's formatter and is not mapped.
            "to_timestamp" if args.len() == 1 => {
                ctx.note("fn:to_timestamp", Tier::Exact, "FROM_UNIXTIME", None)?;
                Ok(format!("FROM_UNIXTIME({})", args[0]))
            }
            // to_char(ts, 'YYYY-MM-DD HH24:MI:SS' and the like): DATE_FORMAT with the same pieces.
            "to_char" if unisql_core::target::to_char_format(f, ["%Y", "%m", "%d", "%H", "%i", "%s"]).is_some() => {
                let fmt = unisql_core::target::to_char_format(f, ["%Y", "%m", "%d", "%H", "%i", "%s"])
                    .unwrap_or_default();
                ctx.note("fn:to_char", Tier::Exact, "DATE_FORMAT", None)?;
                Ok(format!("DATE_FORMAT({}, '{fmt}')", args[0]))
            }
            "to_char" => Err(ctx.refuse(
                "fn:to_char.format",
                "to_char: only literal formats of YYYY, MM, DD, HH24, MI, SS and - / : . , space are mapped",
                &["format in the application"],
            )),
            // date_trunc('<field>', ts): the timestamp text cut at the field, read back as a DATETIME.
            "date_trunc" if args.len() == 2 && unisql_core::target::date_trunc_field(f).is_some() => {
                let field = unisql_core::target::date_trunc_field(f).unwrap_or_default();
                let fmt = match field.as_str() {
                    "year" => "%Y-01-01 00:00:00",
                    "month" => "%Y-%m-01 00:00:00",
                    "day" => "%Y-%m-%d 00:00:00",
                    "hour" => "%Y-%m-%d %H:00:00",
                    "minute" => "%Y-%m-%d %H:%i:00",
                    // The Monday of the ISO week, then the day cut; the first month of the quarter.
                    "week" | "quarter" => "",
                    _ => {
                        return Err(ctx.refuse(
                            &format!("fn:date_trunc.{field}"),
                            &format!("no verified mapping for date_trunc('{field}', ..) on MySQL/MariaDB yet"),
                            &["truncate in the application"],
                        ));
                    }
                };
                ctx.note(
                    format!("fn:date_trunc.{field}"),
                    Tier::Exact,
                    "DATE_FORMAT then CAST AS DATETIME",
                    None,
                )?;
                let ts = &args[1];
                Ok(match field.as_str() {
                    "week" => format!(
                        "CAST(DATE_FORMAT(DATE_SUB({ts}, INTERVAL WEEKDAY({ts}) DAY), '%Y-%m-%d 00:00:00') AS DATETIME)"
                    ),
                    "quarter" => format!(
                        "CAST(CONCAT(YEAR({ts}), '-', LPAD(QUARTER({ts}) * 3 - 2, 2, '0'), '-01 00:00:00') AS DATETIME)"
                    ),
                    _ => format!("CAST(DATE_FORMAT({ts}, '{fmt}') AS DATETIME)"),
                })
            }
            // A MySQL/MariaDB source's own date/time functions are the identity.
            "utc_timestamp" | "utc_date" | "datediff" | "date" | "concat_ws" | "left" | "right"
            | "lpad" | "rpad" | "repeat" | "reverse"
                if matches!(ctx.source, "mysql" | "mariadb") =>
            {
                ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
                Ok(call(n, args))
            }
            "ceil" | "floor" if args.len() == 1 => {
                ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
                Ok(format!("{}({})", n.to_uppercase(), args[0]))
            }
            "strpos" if args.len() == 2 => {
                ctx.note(
                    "fn:strpos",
                    Tier::Conditional,
                    "INSTR compares by the column collation: exact only under a binary (_bin) collation",
                    Some(Allowance::TextCollation),
                )?;
                Ok(format!("INSTR({}, {})", args[0], args[1]))
            }
            "current_date" => {
                ctx.note("fn:current_date", Tier::Exact, "the date in the session time zone", None)?;
                Ok("CURRENT_DATE".to_string())
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
            // A MySQL/MariaDB source's own SUBSTRING is the identity (its start-position rules are
            // the target's too).
            "substr" | "substring" if matches!(ctx.source, "mysql" | "mariadb") => {
                ctx.note("fn:substr", Tier::Exact, "", None)?;
                Ok(call("substring", args))
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
                "fn:make_interval.standalone",
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
            SqlType::Numeric {
                precision: Some(p),
                scale,
            } => format!("DECIMAL({p}, {})", scale.unwrap_or(0)),
            SqlType::Varchar(Some(n)) => format!("CHAR({n})"),
            SqlType::Text | SqlType::Varchar(None) => "CHAR".to_string(),
            SqlType::Date => "DATE".to_string(),
            SqlType::Timestamp { with_tz: false } => "DATETIME".to_string(),
            other => {
                return Err(ctx.refuse(
                    &format!("cast.to_{}", other.leaf()),
                    &format!("CAST to {other:?} has no MySQL equivalent that preserves meaning"),
                    &["store the value in a column of the right type and convert in the application"],
                ));
            }
        };
        ctx.note(
            format!("cast.to_{}", ty.leaf()),
            Tier::Conditional,
            "MySQL casts leniently: a malformed value becomes 0 or a prefix (with a warning) instead of an error",
            Some(Allowance::LenientCast),
        )?;
        Ok(format!("CAST({expr} AS {name})"))
    }

    fn rewrite_query(&self, q: &Query, ctx: &mut Ctx) -> R<Option<Query>> {
        if let SetExpr::Select(s) = &q.body
            && matches!(s.distinct, Distinct::On(_))
        {
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
                Err(c) => Err(ctx.refuse(c.leaf, &c.why, &["list the columns explicitly"])),
            };
        }
        Ok(None)
    }

    fn feature(&self, name: &'static str, ctx: &mut Ctx) -> R<()> {
        match name {
            "window_function" => ctx.note(
                name,
                Tier::Exact,
                "requires MySQL >= 8.0 / MariaDB >= 10.2",
                None,
            ),
            "recursive_cte" => ctx.note(
                name,
                Tier::Exact,
                "requires MySQL >= 8.0 / MariaDB >= 10.2",
                None,
            ),
            "filter_clause" => Err(ctx.refuse(
                "filter_clause",
                "MySQL has no FILTER (WHERE ..) on aggregates",
                &["use SUM(CASE WHEN .. THEN 1 ELSE 0 END)"],
            )),
            "named_arg" => Err(ctx.refuse(
                "named_arg",
                "MySQL functions have no named-argument notation",
                &[],
            )),
            "lateral" => match self.flavor {
                Flavor::MariaDb => Err(ctx.refuse(
                    "lateral",
                    "MariaDB has no LATERAL: a subquery in FROM cannot refer to the tables to its left",
                    &["rewrite as a correlated subquery in the select list, or a window function over a join"],
                )),
                Flavor::MySql => ctx.note(name, Tier::Exact, "requires MySQL >= 8.0.14", None),
            },
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
                    "trim.multi_char",
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
                        if matches!(other, "months" | "years") {
                            "fn:make_interval.month_year"
                        } else {
                            "fn:make_interval"
                        },
                        &format!("make_interval({other} => ..): month/year arithmetic differs between the engines"),
                        &[],
                    ));
                }
            };
            let v = match mult {
                Some(m) => format!("({value}) * {m}"),
                None => value.clone(),
            };
            out = format!(
                "({out} {} INTERVAL ({v}) {u})",
                if minus { "-" } else { "+" }
            );
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

    fn group_by_rollup(&self, keys: &[String], ctx: &mut Ctx) -> R<String> {
        ctx.note("group_by.rollup", Tier::Exact, "", None)?;
        Ok(format!("{} WITH ROLLUP", keys.join(", ")))
    }

    fn distinct_on(&self, ctx: &mut Ctx) -> R<()> {
        // Reached only if a DISTINCT ON survived rewrite_query, which it cannot.
        Err(ctx.refuse("distinct_on", "unexpected DISTINCT ON", &[]))
    }

    fn at_time_zone(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "at_time_zone",
            "MySQL/MariaDB convert zones with CONVERT_TZ, which needs loaded time-zone tables and converts from the session zone, not from timestamptz to a timestamp",
            &["convert in the application, or store UTC"],
        ))
    }

    fn array_subquery(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "array_subquery",
            "MySQL/MariaDB has no array type",
            &["select the rows and collect them in the application"],
        ))
    }

    fn null_safe_eq(&self, lhs: &str, rhs: &str, ctx: &mut Ctx) -> R<String> {
        ctx.note("null_safe_eq", Tier::Exact, "", None)?;
        Ok(format!("{lhs} <=> {rhs}"))
    }

    /// Verified on MySQL 8.4 and MariaDB 11.4 against PostgreSQL's results: year, month, day, hour
    /// and minute are the same fields of the same value; the epoch of a timestamp is
    /// `UNIX_TIMESTAMP` in whole seconds. Other fields (dow, week, second with its fraction ...) and
    /// the epoch of an interval have no verified mapping.
    /// `SYSDATE(3)` is read at the call (not at statement start, unlike `NOW()`), as
    /// `clock_timestamp()` is.
    fn epoch_of_clock(&self, ctx: &mut Ctx) -> R<String> {
        ctx.note("extract.epoch", Tier::Exact, "", None)?;
        Ok("UNIX_TIMESTAMP(SYSDATE(3))".to_string())
    }

    fn extract(&self, field: &str, expr: &str, interval: bool, ctx: &mut Ctx) -> R<String> {
        let leaf = unisql_core::target::extract_leaf(field, interval);
        match field {
            "year" | "month" | "day" | "hour" | "minute" => {
                ctx.note(leaf, Tier::Exact, "", None)?;
                Ok(format!("EXTRACT({} FROM {expr})", field.to_uppercase()))
            }
            // DAYOFWEEK is 1 = Sunday, WEEKDAY is 0 = Monday; WEEK(x, 3) is the ISO 8601 week.
            "dow" | "isodow" | "doy" | "quarter" | "week" => {
                ctx.note(leaf, Tier::Exact, "", None)?;
                Ok(match field {
                    "dow" => format!("(DAYOFWEEK({expr}) - 1)"),
                    "isodow" => format!("(WEEKDAY({expr}) + 1)"),
                    "doy" => format!("DAYOFYEAR({expr})"),
                    "quarter" => format!("QUARTER({expr})"),
                    _ => format!("WEEK({expr}, 3)"),
                })
            }
            "epoch" if !interval => {
                ctx.note(
                    leaf,
                    Tier::Conditional,
                    "UNIX_TIMESTAMP gives whole seconds; PostgreSQL's epoch has the fraction",
                    Some(Allowance::EpochWholeSeconds),
                )?;
                Ok(format!("UNIX_TIMESTAMP({expr})"))
            }
            _ => Err(ctx.refuse(
                &leaf,
                &format!("MySQL/MariaDB: no verified mapping for EXTRACT({field} ..) yet"),
                &["compute the field in the application"],
            )),
        }
    }

    /// Seconds between two timestamps: microsecond difference over a million (the fraction is kept).
    fn epoch_of_difference(&self, a: &str, b: &str, ctx: &mut Ctx) -> R<Option<String>> {
        ctx.note(
            "extract.epoch.interval",
            Tier::Exact,
            "TIMESTAMPDIFF in microseconds",
            None,
        )?;
        Ok(Some(format!(
            "(TIMESTAMPDIFF(MICROSECOND, {b}, {a}) / 1000000)"
        )))
    }

    fn array_literal(&self, _items: &[String], ctx: &mut Ctx) -> R<String> {
        Err(ctx.refuse(
            "array_literal",
            "MySQL/MariaDB has no array type",
            &["use an IN (..) list, or a JSON array"],
        ))
    }

    fn series_as_recursive_cte(&self) -> bool {
        true
    }

    fn table_function(&self, name: &str, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            &format!("table_function.{name}"),
            &format!(
                "MySQL/MariaDB has no verified mapping for the table-valued function {name}()"
            ),
            &["generate the rows in the application, or use a recursive CTE for generate_series"],
        ))
    }

    fn data_modifying_cte(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "cte.data_modifying",
            "MySQL/MariaDB has no data-modifying CTE (WITH x AS (INSERT/UPDATE/DELETE ..)): probed, a syntax error",
            &["run the statements separately in one transaction"],
        ))
    }

    /// Probed: MySQL 8.4 has `WITH .. UPDATE|DELETE` but not `WITH .. INSERT` (the WITH goes inside
    /// the INSERT's SELECT there); MariaDB 11.4 has none of the three.
    fn with_statement(&self, kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        let refused = match self.flavor {
            Flavor::MariaDb => true,
            Flavor::MySql => kind == StmtKind::Insert,
        };
        if refused {
            return Err(ctx.refuse(
                unisql_core::target::with_leaf(kind),
                &format!(
                    "{} has no WITH before this statement (probed: a syntax error)",
                    if self.flavor == Flavor::MariaDb {
                        "MariaDB"
                    } else {
                        "MySQL"
                    }
                ),
                &["put the WITH inside the INSERT's SELECT, or use a derived table"],
            ));
        }
        ctx.note(unisql_core::target::with_leaf(kind), Tier::Exact, "", None)
    }

    fn row_lock(&self, lock: &RowLockClause, ctx: &mut Ctx) -> R<()> {
        if self.flavor == Flavor::MariaDb {
            // Probed on MariaDB 11.4: `FOR SHARE` and `FOR UPDATE OF t` are syntax errors there
            // (`LOCK IN SHARE MODE` is its spelling of a shared lock).
            if lock.strength == RowLock::Share {
                return Err(ctx.refuse(
                    "row_lock.share",
                    "MariaDB has no FOR SHARE (it spells a shared lock LOCK IN SHARE MODE)",
                    &["use FOR UPDATE, or LOCK IN SHARE MODE in the target's own text"],
                ));
            }
            if lock.of.is_some() {
                return Err(ctx.refuse(
                    "row_lock.of",
                    "MariaDB has no FOR UPDATE OF <table>",
                    &["lock all tables of the query, or split the query"],
                ));
            }
        }
        unisql_core::target::note_row_lock_leaves(lock, ctx)
    }

    fn returning(&self, kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        match (self.flavor, kind) {
            (Flavor::MariaDb, StmtKind::Update) => Err(ctx.refuse(
                "returning.update",
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
                &[
                    "select the row back with a second statement",
                    "use MariaDB >= 10.5 for INSERT/DELETE",
                ],
            )),
        }
    }

    fn needs_conflict_key(&self) -> bool {
        false
    }

    fn upsert_where_as_if(&self) -> bool {
        true
    }

    fn replace_keyword(&self) -> Option<&'static str> {
        Some("REPLACE INTO")
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
            // The printer folds the WHERE into SET (`upsert_where_as_if`) and passes none.
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
                return Err(ctx.refuse(
                    "upsert",
                    "DO NOTHING needs at least one inserted column",
                    &[],
                ));
            }
            Ok(format!(
                " ON DUPLICATE KEY UPDATE {first_insert_col} = {first_insert_col}"
            ))
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
                ctx.note(
                    "offset",
                    Tier::Emulated,
                    "MySQL requires LIMIT with OFFSET; 2^64-1 means unbounded",
                    None,
                )?;
                Ok(format!(" LIMIT 18446744073709551615 OFFSET {o}"))
            }
        }
    }

    fn order_item(&self, expr: &str, desc: bool, nulls_first: bool, ctx: &mut Ctx) -> R<String> {
        if ctx.order_non_text {
            ctx.note("order_by", Tier::Exact, "non-text column", None)?;
        } else {
            ctx.note(
                "order_by",
                Tier::Conditional,
                "text ordering follows the column collation",
                Some(Allowance::TextCollation),
            )?;
        }
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

    fn binary_override(
        &self,
        op: BinaryOp,
        l: &str,
        r: &str,
        numeric: bool,
        ctx: &mut Ctx,
    ) -> R<Option<String>> {
        match op {
            BinaryOp::Concat => {
                ctx.note(
                    "concat",
                    Tier::Exact,
                    "|| is OR in MySQL, so CONCAT() is used",
                    None,
                )?;
                Ok(Some(format!("CONCAT({l}, {r})")))
            }
            BinaryOp::IntDivide => {
                ctx.note("division_integer", Tier::Exact, "DIV", None)?;
                Ok(Some(format!("{l} DIV {r}")))
            }
            BinaryOp::BitXor => Ok(Some(format!("{l} ^ {r}"))),
            BinaryOp::Xor => Ok(Some(format!("{l} XOR {r}"))),
            // A MySQL/MariaDB source's `/` is already MySQL division (exact decimal).
            BinaryOp::Divide if matches!(ctx.source, "mysql" | "mariadb") => Ok(None),
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
        if matches!(
            op,
            BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::ShiftLeft
                | BinaryOp::ShiftRight
        ) {
            if matches!(ctx.source, "mysql" | "mariadb") {
                return ctx.note("bitwise", Tier::Exact, "", None);
            }

            return ctx.note(
                "bitwise",
                Tier::Conditional,
                "MySQL bitwise operators work on unsigned 64-bit integers, PostgreSQL and SQLite on signed ones: exact only for non-negative operands and shift counts below the integer width",
                Some(Allowance::BitwiseNonNegativeIntegers),
            );
        }
        if op == BinaryOp::Xor {
            return ctx.note("xor", Tier::Exact, "", None);
        }
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

    fn array_membership(
        &self,
        lhs: &str,
        ph: &str,
        negated: bool,
        n: u32,
        ctx: &mut Ctx,
    ) -> R<String> {
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

    fn dml_sources(&self) -> DmlSources {
        DmlSources::Join
    }

    fn dml_limit(&self) -> DmlLimit {
        DmlLimit::Native
    }

    fn nulls_order(&self, _desc: bool, _nulls_first: bool, _ctx: &mut Ctx) -> R<String> {
        Ok(String::new()) // handled in order_item
    }
}
