//! SQLite dialect module: a writer (the [`Sqlite`] target). A reader is a later slice.
//!
//! Every deviation from PostgreSQL semantics is a named [`Allowance`] or a refusal; the
//! differential tests in `unisql-differential` are what justify each tier. Minimum engine
//! version assumed by the tiers: SQLite 3.35 (`RETURNING`), noted per construct.

use unisql_core::ast::*;
use unisql_core::report::{Allowance, Tier, Untranslatable};
use unisql_core::target::{generic_call, Ctx};
use unisql_core::Target;

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

    fn like(&self, lhs: &str, rhs: &str, negated: bool, ci: bool, ctx: &mut Ctx) -> R<String> {
        let not = if negated { "NOT " } else { "" };
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
            Ok(format!("lower({lhs}) {not}LIKE lower({rhs}) ESCAPE '\\'"))
        } else {
            ctx.note(
                "like",
                Tier::Conditional,
                "SQLite LIKE is ASCII case-insensitive unless PRAGMA case_sensitive_like=ON; \
                 an ESCAPE clause is added to match PostgreSQL's default escape character",
                Some(Allowance::LikeCaseSensitivityIsConnectionSetting),
            )?;
            Ok(format!("{lhs} {not}LIKE {rhs} ESCAPE '\\'"))
        }
    }

    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        let n = f.name.as_str();
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
                    "-> group_concat(value, separator); element order unspecified on both",
                    None,
                )?;
                Ok(generic_call("group_concat", args, f.distinct, false))
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

    fn returning(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "requires SQLite >= 3.35", None)
    }

    fn upsert(&self, has_where: bool, ctx: &mut Ctx) -> R<()> {
        ctx.note(
            if has_where { "upsert_where" } else { "upsert" },
            Tier::Exact,
            "requires SQLite >= 3.24",
            None,
        )
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
