// SPDX-License-Identifier: MIT OR Apache-2.0
//! PostgreSQL dialect module: a reader (text -> IR) and a writer ([`Postgres`] target).

use unisql_core::ast::*;
use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};
use unisql_core::target::{Ctx, PORTABLE_FUNCTIONS, generic_call_ordered};
use unisql_core::{Read, ReadError, Target};

type R<T> = Result<T, Untranslatable>;

/// Functions PostgreSQL has with the meaning a foreign source's reader gives them.
const FROM_OTHER: &[&str] = &[
    "string_agg",
    "now",
    "ltrim",
    "rtrim",
    "trim",
    "char_length",
    "octet_length",
    "coalesce",
    "make_interval",
];

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
        if ctx.source != "postgres" && !PORTABLE_FUNCTIONS.contains(&n) && !FROM_OTHER.contains(&n)
        {
            return Err(ctx.refuse(
                &format!("fn:{n}"),
                "no verified mapping for this function on PostgreSQL yet",
                &[],
            ));
        }
        // MySQL's month/year arithmetic clamps at the end of a month by its own rules; PostgreSQL
        // has the same call but the two are not verified to agree, so only the fixed-length units
        // cross from another dialect.
        if n == "make_interval" && ctx.source != "postgres" {
            for a in &f.args {
                if let FunctionArg::Named { name, .. } = a
                    && matches!(name.as_str(), "months" | "years")
                {
                    return Err(ctx.refuse(
                        "fn:make_interval.month_year",
                        &format!("make_interval({name} => ..): month/year arithmetic differs between the engines"),
                        &["use secs/mins/hours/days/weeks"],
                    ));
                }
            }
        }
        ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
        let order = ctx.agg_order.take();
        Ok(generic_call_ordered(
            n,
            args,
            f.distinct,
            f.star,
            order.as_deref(),
        ))
    }

    fn array_membership(
        &self,
        lhs: &str,
        ph: &str,
        negated: bool,
        n: u32,
        ctx: &mut Ctx,
    ) -> R<String> {
        ctx.note("array_param", Tier::Exact, "native array parameter", None)?;
        ctx.bind_array_param(n, ParamBinding::NativeArray);
        Ok(if negated {
            format!("{lhs} <> ALL({ph})")
        } else {
            format!("{lhs} = ANY({ph})")
        })
    }

    fn binary_op(&self, op: BinaryOp, numeric: bool, ctx: &mut Ctx) -> R<()> {
        if op == BinaryOp::IntDivide {
            return ctx.note(
                "division_integer",
                Tier::Conditional,
                "DIV becomes `/`, which is integer division only if both operands are integers",
                Some(Allowance::IntegerDivisionAssumed),
            );
        }
        if !ctx.lenient_source() {
            return Ok(());
        }
        match op {
            BinaryOp::Divide | BinaryOp::Modulo | BinaryOp::IntDivide => ctx.note(
                "division",
                Tier::Conditional,
                "the source yields NULL on a zero divisor; PostgreSQL raises an error",
                Some(Allowance::DivisionByZeroIsNull),
            ),
            BinaryOp::Eq | BinaryOp::NotEq if !numeric && ctx.source != "sqlite" => ctx.note(
                "text_comparison",
                Tier::Conditional,
                "text equality follows the source column collation; the target compares by its own",
                Some(Allowance::TextCollation),
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
        if ctx.lenient_source() && !numeric {
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
        if ctx.lenient_source() {
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
