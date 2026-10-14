// SPDX-License-Identifier: MIT OR Apache-2.0
//! SQLite dialect module: a reader (text -> IR) and a writer (the [`Sqlite`] target).
//!
//! Every deviation from PostgreSQL semantics is a named [`Allowance`] or a refusal; the
//! differential tests in `unisql-differential` are what justify each tier. Minimum engine
//! version assumed by the tiers: SQLite 3.35 (`RETURNING`), noted per construct.

use unisql_core::ast::*;
use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};
use unisql_core::rewrite;
use unisql_core::target::{Ctx, Escape, StmtKind, generic_call, generic_call_ordered};
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

    fn regexp(&self, _lhs: &str, _rhs: &str, _negated: bool, ctx: &mut Ctx) -> R<String> {
        // SQLite parses `REGEXP` but ships no implementation: it calls an application-defined
        // function that a plain connection does not have, so the statement would fail at run time.
        Err(ctx.refuse(
            "regexp",
            "SQLite has no built-in regular-expression matcher (REGEXP needs an application-defined function)",
            &["LIKE or GLOB for simple patterns"],
        ))
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
                            "row_number() OVER (PARTITION BY keys ORDER BY ..) = 1 in a derived table; \
                             without ORDER BY the surviving row per key is arbitrary on both engines",
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
                        if matches!(other, "months" | "years") {
                            "fn:make_interval.month_year"
                        } else {
                            "fn:make_interval"
                        },
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
        if n == "current_date" {
            if ctx.source == "sqlite" {
                ctx.note("fn:current_date", Tier::Exact, "", None)?;
            } else {
                ctx.note(
                    "fn:current_date",
                    Tier::Conditional,
                    "-> CURRENT_DATE; text 'YYYY-MM-DD' of the UTC date, not a date in the session time zone",
                    Some(Allowance::DateAsText),
                )?;
            }
            return Ok("CURRENT_DATE".to_string());
        }
        if ctx.source == "sqlite" {
            if n == "now" {
                // The reader normalised CURRENT_TIMESTAMP to now(); SQLite spells it back.
                ctx.note("fn:now", Tier::Exact, "", None)?;
                return Ok("CURRENT_TIMESTAMP".to_string());
            }
            // Same dialect in and out: the reader accepted the name, so this is the identity.
            ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
            let order = ctx.agg_order.take();
            return Ok(generic_call_ordered(
                n,
                args,
                f.distinct,
                f.star,
                order.as_deref(),
            ));
        }
        match n {
            "utc_timestamp" => {
                let (text, allow2) = match unisql_core::target::utc_timestamp_precision(f) {
                    Some(0) => ("datetime('now')", None),
                    Some(6) => (
                        "strftime('%Y-%m-%d %H:%M:%f', 'now')",
                        Some(Allowance::TimestampMillisecondResolution),
                    ),
                    _ => {
                        return Err(ctx.refuse(
                            "fn:utc_timestamp.precision",
                            "only UTC_TIMESTAMP() and UTC_TIMESTAMP(6) are verified",
                            &["use UTC_TIMESTAMP() or UTC_TIMESTAMP(6)"],
                        ));
                    }
                };
                ctx.note(
                    "fn:utc_timestamp",
                    Tier::Conditional,
                    "text 'YYYY-MM-DD HH:MM:SS[.fff]' in UTC, not a timestamp type",
                    Some(Allowance::TimestampAsText),
                )?;
                if let Some(a) = allow2 {
                    ctx.note(
                        "fn:utc_timestamp",
                        Tier::Conditional,
                        "SQLite's %f has millisecond resolution; MySQL's UTC_TIMESTAMP(6) has microseconds",
                        Some(a),
                    )?;
                }
                return Ok(text.to_string());
            }
            "utc_date" if args.is_empty() => {
                ctx.note(
                    "fn:utc_date",
                    Tier::Conditional,
                    "text 'YYYY-MM-DD' of the UTC date, not a date type",
                    Some(Allowance::DateAsText),
                )?;
                return Ok("date('now')".to_string());
            }
            "datediff" if args.len() == 2 => {
                ctx.note(
                    "fn:datediff",
                    Tier::Conditional,
                    "operands are timestamp text; whole days between their date parts",
                    Some(Allowance::TimestampAsText),
                )?;
                return Ok(format!(
                    "CAST(julianday(date({})) - julianday(date({})) AS INTEGER)",
                    args[0], args[1]
                ));
            }
            "concat_ws" if args.len() >= 2 => {
                ctx.note("fn:concat_ws", Tier::Exact, "requires SQLite >= 3.44", None)?;
                return Ok(format!("concat_ws({})", args.join(", ")));
            }
            "left" if args.len() == 2 && unisql_core::target::literal_count(f, 1).is_some() => {
                ctx.note("fn:left", Tier::Exact, "-> substr(s, 1, n)", None)?;
                return Ok(format!("substr({}, 1, {})", args[0], args[1]));
            }
            "left" if args.len() == 2 => {
                return Err(ctx.refuse(
                    "fn:left.length",
                    "LEFT(s, n) is mapped only for a literal n >= 0",
                    &["use a literal non-negative count"],
                ));
            }
            "right" if args.len() == 2 => {
                return match unisql_core::target::literal_count(f, 1) {
                    Some(0) => {
                        ctx.note("fn:right", Tier::Exact, "-> substr(s, 1, 0)", None)?;
                        Ok(format!("substr({}, 1, 0)", args[0]))
                    }
                    Some(_) => {
                        ctx.note("fn:right", Tier::Exact, "-> substr(s, -n)", None)?;
                        Ok(format!("substr({}, -{})", args[0], args[1]))
                    }
                    None => Err(ctx.refuse(
                        "fn:right.length",
                        "RIGHT(s, n) is mapped only for a literal n >= 0",
                        &["use a literal non-negative count"],
                    )),
                };
            }
            "repeat" if args.len() == 2 && unisql_core::target::literal_count(f, 1).is_some() => {
                ctx.note(
                    "fn:repeat",
                    Tier::Emulated,
                    "-> replace(hex(zeroblob(n)), '00', s)",
                    None,
                )?;
                return Ok(format!(
                    "replace(hex(zeroblob({})), '00', {})",
                    args[1], args[0]
                ));
            }
            "date" if args.len() == 1 => {
                ctx.note(
                    "fn:date",
                    Tier::Conditional,
                    "text 'YYYY-MM-DD', not a date type",
                    Some(Allowance::DateAsText),
                )?;
                return Ok(format!("date({})", args[0]));
            }
            _ => {}
        }
        if f.over.is_some()
            && matches!(
                n,
                "row_number"
                    | "rank"
                    | "dense_rank"
                    | "percent_rank"
                    | "cume_dist"
                    | "ntile"
                    | "lag"
                    | "lead"
                    | "first_value"
                    | "last_value"
                    | "nth_value"
            )
        {
            ctx.note(format!("fn:{n}"), Tier::Exact, "window function", None)?;
            return Ok(generic_call(n, args, false, false));
        }
        match n {
            "md5" | "sha256" | "sha1" | "sha512" | "encode" | "decode" | "convert_to"
            | "convert_from" => {
                return Err(ctx.refuse(
                    &format!("fn:{n}"),
                    "SQLite has no built-in hashing/encoding functions; the result would depend on a \
                     host-registered function or extension, which UniSQL cannot assume",
                    &["hash in the application", "register the function and use a raw(sqlite, ..) fragment"],
                ));
            }
            "current_schema" | "current_database" | "current_user" | "session_user"
            | "to_regclass" | "pg_backend_pid" | "set_config" | "current_setting" => {
                return Err(ctx.refuse(
                    &format!("fn:{n}"),
                    "PostgreSQL catalog/session introspection has no SQLite equivalent",
                    &["query sqlite_schema (a different, SQLite-specific statement)"],
                ));
            }
            "make_interval" => {
                return Err(ctx.refuse(
                    "fn:make_interval.standalone",
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
            "count" | "sum" | "avg" | "coalesce" | "nullif" | "abs" | "length" | "replace"
            | "trim" | "ltrim" | "rtrim" | "substr" | "ifnull" => {
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
                    return Err(ctx.refuse(
                        "fn:octet_length",
                        "octet_length takes one argument",
                        &[],
                    ));
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
                // SQLite: a DISTINCT aggregate takes exactly one argument (the separator is ',').
                let args = if f.distinct {
                    if args[1] != "','" {
                        return Err(ctx.refuse(
                            "fn:string_agg",
                            "SQLite group_concat(DISTINCT ..) cannot take a separator",
                            &["aggregate without DISTINCT, or use the default ',' separator"],
                        ));
                    }
                    &args[..1]
                } else {
                    args
                };
                Ok(generic_call_ordered(
                    "group_concat",
                    args,
                    f.distinct,
                    false,
                    order.as_deref(),
                ))
            }
            // to_timestamp(seconds): datetime(x, 'unixepoch'), text in UTC, whole seconds.
            "to_timestamp" if args.len() == 1 => {
                ctx.note(
                    "fn:to_timestamp",
                    Tier::Conditional,
                    "datetime(x, 'unixepoch'): text YYYY-MM-DD HH:MM:SS in UTC",
                    Some(Allowance::TimestampAsText),
                )?;
                ctx.note(
                    "fn:to_timestamp",
                    Tier::Conditional,
                    "a fraction of a second is dropped",
                    Some(Allowance::TimestampWholeSeconds),
                )?;
                Ok(format!("datetime({}, 'unixepoch')", args[0]))
            }
            // date_trunc('<field>', ts): the timestamp text cut at the field.
            "date_trunc"
                if args.len() == 2 && unisql_core::target::date_trunc_field(f).is_some() =>
            {
                let field = unisql_core::target::date_trunc_field(f).unwrap_or_default();
                let fmt = match field.as_str() {
                    "year" => "%Y-01-01 00:00:00",
                    "month" => "%Y-%m-01 00:00:00",
                    "day" => "%Y-%m-%d 00:00:00",
                    "hour" => "%Y-%m-%d %H:00:00",
                    "minute" => "%Y-%m-%d %H:%M:00",
                    _ => {
                        return Err(ctx.refuse(
                            &format!("fn:date_trunc.{field}"),
                            &format!(
                                "no verified mapping for date_trunc('{field}', ..) on SQLite yet"
                            ),
                            &["truncate in the application"],
                        ));
                    }
                };
                ctx.note(
                    format!("fn:date_trunc.{field}"),
                    Tier::Conditional,
                    "strftime cuts the timestamp text; the result is text",
                    Some(Allowance::TimestampAsText),
                )?;
                Ok(format!("strftime('{fmt}', {})", args[1]))
            }
            // Same construction as on MySQL, from randomblob(): verified over 2000 values.
            "gen_random_uuid" => {
                ctx.note(
                    "fn:gen_random_uuid",
                    Tier::Emulated,
                    "assembled from randomblob(): version 4, variant 10xx",
                    None,
                )?;
                Ok(
                    "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || \
                    substr(hex(randomblob(2)), 2) || '-' || \
                    substr('89ab', 1 + (abs(random()) % 4), 1) || \
                    substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))"
                        .to_string(),
                )
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
                &[
                    "add a mapping with a differential test",
                    "use a raw(sqlite, ..) fragment",
                ],
            )),
        }
    }

    fn cast(&self, expr: &str, ty: &SqlType, ctx: &mut Ctx) -> R<String> {
        let name = match ty {
            SqlType::SmallInt | SqlType::Integer | SqlType::BigInt => "INTEGER",
            SqlType::Real | SqlType::Double => "REAL",
            SqlType::Numeric { .. } => "NUMERIC",
            SqlType::Text | SqlType::Varchar(_) => "TEXT",
            // A timestamp without zone is the text `YYYY-MM-DD HH:MM:SS` already (every timestamp
            // is text here), so the cast keeps the text. Verified against PostgreSQL's value
            // through its epoch on fixed timestamps.
            SqlType::Timestamp { with_tz: false } => {
                ctx.note(
                    "cast.to_timestamp",
                    Tier::Conditional,
                    "a timestamp is text on SQLite: the cast keeps the text",
                    Some(Allowance::TimestampAsText),
                )?;
                ctx.note(
                    "cast.to_timestamp",
                    Tier::Conditional,
                    "SQLite casts leniently: a malformed value is kept instead of raising an error",
                    Some(Allowance::LenientCast),
                )?;
                return Ok(format!("CAST({expr} AS TEXT)"));
            }
            other => {
                return Err(ctx.refuse(
                    &format!("cast.to_{}", other.leaf()),
                    &format!("CAST to {other:?} has no SQLite equivalent that preserves meaning"),
                    &["store as TEXT/INTEGER and convert in the application"],
                ));
            }
        };
        ctx.note(
            format!("cast.to_{}", ty.leaf()),
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

    fn row_lock(&self, _lock: &RowLockClause, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            unisql_core::target::row_lock_leaf(_lock),
            "SQLite has no row-level locks (it locks the whole database)",
            &["use BEGIN IMMEDIATE for a write transaction"],
        ))
    }

    fn at_time_zone(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "at_time_zone",
            "SQLite has no time zones: its datetimes are text in UTC",
            &["convert in the application, or store UTC"],
        ))
    }

    fn array_subquery(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "array_subquery",
            "SQLite has no array type",
            &["select the rows and collect them in the application"],
        ))
    }

    fn null_safe_eq(&self, lhs: &str, rhs: &str, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "null_safe_eq",
            Tier::Exact,
            "SQLite `IS` compares like `<=>`",
            None,
        )?;
        Ok(format!("{lhs} IS {rhs}"))
    }

    /// Verified on SQLite 3.53 against PostgreSQL's results: the fields are read from the timestamp
    /// text with `strftime`; the epoch of a timestamp is `strftime('%s')`, whole seconds. Other fields
    /// and the epoch of an interval have no verified mapping.
    fn extract(&self, field: &str, expr: &str, interval: bool, ctx: &mut Ctx) -> R<String> {
        let leaf = unisql_core::target::extract_leaf(field, interval);
        let fmt = match field {
            "year" => "%Y",
            "month" => "%m",
            "day" => "%d",
            "hour" => "%H",
            "minute" => "%M",
            "epoch" if !interval => "%s",
            _ => {
                return Err(ctx.refuse(
                    &leaf,
                    &format!("SQLite: no verified mapping for EXTRACT({field} ..) yet"),
                    &["compute the field in the application"],
                ));
            }
        };
        if fmt == "%s" {
            ctx.note(
                leaf,
                Tier::Conditional,
                "strftime('%s') gives whole seconds; PostgreSQL's epoch has the fraction",
                Some(Allowance::EpochWholeSeconds),
            )?;
        } else {
            ctx.note(
                leaf,
                Tier::Conditional,
                "the field is read from the timestamp's text (YYYY-MM-DD HH:MM:SS, UTC)",
                Some(Allowance::TimestampAsText),
            )?;
        }
        Ok(format!("CAST(strftime('{fmt}', {expr}) AS INTEGER)"))
    }

    /// Whole seconds between two timestamp texts.
    fn epoch_of_difference(&self, a: &str, b: &str, ctx: &mut Ctx) -> R<Option<String>> {
        ctx.note(
            "extract.epoch.interval",
            Tier::Conditional,
            "whole seconds between the two timestamp texts (strftime('%s'))",
            Some(Allowance::EpochWholeSeconds),
        )?;
        Ok(Some(format!(
            "(CAST(strftime('%s', {a}) AS INTEGER) - CAST(strftime('%s', {b}) AS INTEGER))"
        )))
    }

    fn array_literal(&self, _items: &[String], ctx: &mut Ctx) -> R<String> {
        Err(ctx.refuse(
            "array_literal",
            "SQLite has no array type",
            &["use an IN (..) list, or a JSON array"],
        ))
    }

    fn table_function(&self, name: &str, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            &format!("table_function.{name}"),
            &format!("SQLite has no verified mapping for the table-valued function {name}()"),
            &["generate the rows in the application, or use a recursive CTE for generate_series"],
        ))
    }

    fn data_modifying_cte(&self, ctx: &mut Ctx) -> R<()> {
        Err(ctx.refuse(
            "cte.data_modifying",
            "SQLite has no data-modifying CTE (WITH x AS (INSERT/UPDATE/DELETE ..)): probed, a syntax error",
            &["run the statements separately in one transaction"],
        ))
    }

    fn returning(&self, _kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "requires SQLite >= 3.35", None)
    }

    fn replace_keyword(&self) -> Option<&'static str> {
        Some("INSERT OR REPLACE INTO")
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
        if !do_nothing && conflict_cols.is_empty() {
            return Err(ctx.refuse(
                "upsert.no_conflict_key",
                "the source upsert names no conflict key (MySQL ON DUPLICATE KEY UPDATE fires on any unique key); SQLite needs the key columns, and the schema does not state exactly one unique key for the table",
                &["state the conflict key columns in the statement", "or give the schema the table's unique key (Schema::with_unique_key, or schema_from_ddl)"],
            ));
        }
        ctx.note(
            if where_.is_some() {
                "upsert_where"
            } else {
                "upsert"
            },
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
            "bind the array as JSON text; membership via json_each(). A NULL array reads as empty",
            Some(Allowance::ArrayNullSemantics),
        )?;
        ctx.bind_array_param(n, ParamBinding::JsonArrayText);
        let not = if negated { "NOT " } else { "" };
        Ok(format!("{lhs} {not}IN (SELECT value FROM json_each({ph}))"))
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
        if matches!(
            op,
            BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::ShiftLeft
                | BinaryOp::ShiftRight
        ) {
            if ctx.source == "sqlite" {
                return ctx.note("bitwise", Tier::Exact, "", None);
            }
            if op == BinaryOp::BitXor {
                return Err(ctx.refuse(
                    "bitwise.xor",
                    "SQLite has no bitwise XOR operator (it would need the operands written twice)",
                    &["(a | b) - (a & b) with simple operands"],
                ));
            }
            return ctx.note(
                "bitwise",
                Tier::Conditional,
                "MySQL bitwise operators work on unsigned 64-bit integers, PostgreSQL and SQLite on signed ones: exact only for non-negative operands and shift counts below the integer width",
                Some(Allowance::BitwiseNonNegativeIntegers),
            );
        }
        if op == BinaryOp::Xor {
            return ctx.note(
                "xor",
                Tier::Conditional,
                "XOR is written as <>, which compares truth values only when both operands are booleans or 0/1 comparison results",
                Some(Allowance::XorOperandsBoolean),
            );
        }
        match op {
            BinaryOp::IntDivide => ctx.note(
                "division_integer",
                Tier::Conditional,
                "DIV becomes `/`, which is integer division only if both operands are integers",
                Some(Allowance::IntegerDivisionAssumed),
            ),
            BinaryOp::Eq | BinaryOp::NotEq if !numeric && matches!(ctx.source, "mysql" | "mariadb") => ctx.note(
                "text_comparison",
                Tier::Conditional,
                "text equality follows the source column collation; SQLite compares bytes (BINARY)",
                Some(Allowance::TextCollation),
            ),
            BinaryOp::Divide | BinaryOp::Modulo => ctx.note(
                "division",
                Tier::Conditional,
                "division/modulo by zero is NULL on SQLite (an error on PostgreSQL)",
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
            Ok(if nulls_first {
                " NULLS FIRST"
            } else {
                " NULLS LAST"
            }
            .to_string())
        }
    }
}
