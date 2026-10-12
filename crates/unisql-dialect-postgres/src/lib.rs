//! PostgreSQL dialect module: a reader (text -> IR) and a writer ([`Postgres`] target).

use unisql_core::ast::*;
use unisql_core::report::{Allowance, Tier, Untranslatable};
use unisql_core::target::{generic_call_ordered, Ctx, PORTABLE_FUNCTIONS};
use unisql_core::{Read, ReadError, Target};

type R<T> = Result<T, Untranslatable>;

/// Functions PostgreSQL has with the meaning a foreign source's reader gives them.
const FROM_OTHER: &[&str] = &["string_agg", "now", "ltrim", "rtrim", "trim", "char_length", "octet_length"];

/// Read exactly one PostgreSQL statement.
pub fn read(sql: &str) -> Result<Read, ReadError> {
    unisql_sqlparser_adapter::read(sql, &unisql_sqlparser_adapter::POSTGRES)
}

/// The PostgreSQL target. Every hook keeps the core's defaults, which describe PostgreSQL, apart
/// from the notes that apply when the *source* is another dialect.
#[derive(Clone, Copy, Debug, Default)]
pub struct Postgres;

impl Target for Postgres {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        let n = f.name.as_str();
        if ctx.source != "postgres" && !PORTABLE_FUNCTIONS.contains(&n) && !FROM_OTHER.contains(&n) {
            return Err(ctx.refuse(
                &format!("fn:{n}"),
                "no verified mapping for this function on PostgreSQL yet",
                &[],
            ));
        }
        ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
        let order = ctx.agg_order.take();
        Ok(generic_call_ordered(n, args, f.distinct, f.star, order.as_deref()))
    }

    fn binary_op(&self, op: BinaryOp, numeric: bool, ctx: &mut Ctx) -> R<()> {
        if ctx.source != "sqlite" {
            return Ok(());
        }
        match op {
            BinaryOp::Divide | BinaryOp::Modulo => ctx.note(
                "division",
                Tier::Conditional,
                "the source yields NULL on a zero divisor; PostgreSQL raises an error",
                Some(Allowance::DivisionByZeroIsNull),
            ),
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq if !numeric => ctx.note(
                "ordered_comparison",
                Tier::Conditional,
                "text comparison follows PostgreSQL's collation; the source compares bytes (BINARY)",
                Some(Allowance::TextCollation),
            ),
            _ => Ok(()),
        }
    }

    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {
        if ctx.source == "sqlite" && !numeric {
            return ctx.note(
                "ordered_comparison",
                Tier::Conditional,
                "text comparison follows PostgreSQL's collation; the source compares bytes (BINARY)",
                Some(Allowance::TextCollation),
            );
        }
        Ok(())
    }

    fn nulls_order(&self, desc: bool, nulls_first: bool, ctx: &mut Ctx) -> R<String> {
        if ctx.source == "sqlite" {
            ctx.note(
                "order_by",
                Tier::Conditional,
                "text ordering follows PostgreSQL's collation; the source orders bytes (BINARY)",
                Some(Allowance::TextCollation),
            )?;
        }
        ctx.note("null_ordering", Tier::Exact, "", None)?;
        // PostgreSQL default: NULLs are the largest value (first when descending).
        Ok(if nulls_first == desc {
            String::new()
        } else if nulls_first {
            " NULLS FIRST".to_string()
        } else {
            " NULLS LAST".to_string()
        })
    }
}
