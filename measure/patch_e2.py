def sub(p, a, b, count=1):
    s = open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')
    assert s.count(a) == count, (p, s.count(a), a[:100])
    open(p, 'w', encoding='utf-8', newline='').write(s.replace(a, b))


AST = 'crates/unisql-core/src/ast.rs'
sub(AST, "    Nested(Box<Expr>),\n    /// `TRIM(", """    Nested(Box<Expr>),
    /// `expr = ANY($n)` (or `expr <> ALL($n)` when `negated`): membership in an *array parameter*.
    /// The parameter's binding shape is target-specific; see `Translation::param_bindings`.
    AnyParam { expr: Box<Expr>, param: u32, negated: bool },
    /// `TRIM(""")

RPT = 'crates/unisql-core/src/report.rs'
sub(RPT, """    /// `a / b` becomes integer division (`DIV`), valid only if both operands are integers.""", """    /// An array parameter used for membership: a `NULL` array, `NULL` elements or a `NULL` operand
    /// give an unknown result on the source but a definite true/false on the target.
    ArrayNullSemantics,
    /// `a / b` becomes integer division (`DIV`), valid only if both operands are integers.""")
sub(RPT, """            Allowance::IntegerDivisionAssumed => "integer_division_assumed",""", """            Allowance::ArrayNullSemantics => "array_null_semantics",
            Allowance::IntegerDivisionAssumed => "integer_division_assumed",""")
sub(RPT, """/// A successful translation.""", """/// How the caller must bind one source parameter on the target. The *text* of a statement is not
/// enough when a parameter changes shape between engines (an array that SQLite and MySQL can only
/// take as JSON text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamBinding {
    /// A single value, bound as it would be on the source.
    Scalar,
    /// A native array value (PostgreSQL `int4[]`, `text[]`, ...).
    NativeArray,
    /// A JSON array, bound as text: `[1,3]` or `["a","b"]`.
    JsonArrayText,
}

impl ParamBinding {
    pub fn name(self) -> &'static str {
        match self {
            ParamBinding::Scalar => "scalar",
            ParamBinding::NativeArray => "native_array",
            ParamBinding::JsonArrayText => "json_array_text",
        }
    }
}

/// A successful translation.""")
sub(RPT, """    pub param_order: Vec<u32>,
    pub report: Report,""", """    pub param_order: Vec<u32>,
    /// One entry per source parameter (index `n - 1` is `$n`): how to bind it on the target.
    pub param_bindings: Vec<ParamBinding>,
    pub report: Report,""")
sub('crates/unisql-core/src/lib.rs', "Allowance, Entry, Options, Read, ReadError, ReadNote, Report, Tier, Translation, Untranslatable,",
    "Allowance, Entry, Options, ParamBinding, Read, ReadError, ReadNote, Report, Tier, Translation,\n    Untranslatable,")

TGT = 'crates/unisql-core/src/target.rs'
sub(TGT, "use crate::report::{Allowance, Entry, Options, Report, Tier, Untranslatable};",
    "use crate::report::{Allowance, Entry, Options, ParamBinding, Report, Tier, Untranslatable};")
sub(TGT, "    pub agg_order: Option<String>,\n}", """    pub agg_order: Option<String>,
    /// Parameters used as arrays, with the binding the target requires.
    pub array_params: std::collections::BTreeMap<u32, ParamBinding>,
    /// Parameters used as plain values (a parameter must not be both).
    pub scalar_params: std::collections::BTreeSet<u32>,
}""")
sub(TGT, "in_upsert: false, agg_order: None }", "in_upsert: false, agg_order: None, array_params: Default::default(), scalar_params: Default::default() }")
sub(TGT, "    /// Hook for `[NOT] BETWEEN`.", """    /// Membership in an array parameter: `lhs = ANY(ph)` (or `<> ALL` when `negated`). The target
    /// must record how the caller binds parameter `n` via [`Ctx::bind_array_param`]. **The default
    /// refuses**: a target with no array codec has no honest translation.
    fn array_membership(&self, _lhs: &str, _ph: &str, _negated: bool, n: u32, ctx: &mut Ctx) -> R<String> {
        Err(ctx.refuse(
            "array_param",
            &format!("target `{}` has no codec for an array parameter (${n})", ctx.target),
            &["expand the array into an IN (..) list in the application"],
        ))
    }

    /// Hook for `[NOT] BETWEEN`.""")
# Ctx::bind_array_param
sub(TGT, "    pub fn refuse(&self, construct: &str, reason: &str, alternatives: &[&str]) -> Untranslatable {", """    /// Record that source parameter `n` is an array that the caller must bind as `binding`.
    pub fn bind_array_param(&mut self, n: u32, binding: ParamBinding) {
        self.array_params.insert(n, binding);
    }

    pub fn refuse(&self, construct: &str, reason: &str, alternatives: &[&str]) -> Untranslatable {""")

PRN = 'crates/unisql-core/src/print.rs'
sub(PRN, """    let sql = Printer { t: target }.statement(stmt, &mut ctx)?;
    Ok(Translation { sql, param_count: ctx.max_param, param_order: ctx.param_order, report: ctx.report })""", """    let sql = Printer { t: target }.statement(stmt, &mut ctx)?;
    if let Some(n) = ctx.array_params.keys().find(|n| ctx.scalar_params.contains(n)) {
        return Err(ctx.refuse(
            "array_param",
            &format!("parameter ${n} is used both as an array and as a single value"),
            &[],
        ));
    }
    let param_bindings = (1..=ctx.max_param)
        .map(|n| ctx.array_params.get(&n).copied().unwrap_or(ParamBinding::Scalar))
        .collect();
    Ok(Translation {
        sql,
        param_count: ctx.max_param,
        param_order: ctx.param_order,
        param_bindings,
        report: ctx.report,
    })""")
sub(PRN, "use crate::report::{Options, Translation, Untranslatable};", "use crate::report::{Options, ParamBinding, Translation, Untranslatable};")
sub(PRN, """            Expr::Param(n) => {
                c.max_param = c.max_param.max(*n);
                c.param_order.push(*n);
                self.t.placeholder(*n)
            }""", """            Expr::Param(n) => {
                c.max_param = c.max_param.max(*n);
                c.param_order.push(*n);
                c.scalar_params.insert(*n);
                self.t.placeholder(*n)
            }
            Expr::AnyParam { expr, param, negated } => {
                let lhs = self.operand(expr, c)?;
                c.max_param = c.max_param.max(*param);
                c.param_order.push(*param);
                let ph = self.t.placeholder(*param);
                self.t.array_membership(&lhs, &ph, *negated, *param, c)?
            }""")

# PostgreSQL: native array
PG = 'crates/unisql-dialect-postgres/src/lib.rs'
sub(PG, "use unisql_core::report::{Allowance, Tier, Untranslatable};", "use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};")
sub(PG, "    fn binary_op(&self, op: BinaryOp, numeric: bool, ctx: &mut Ctx) -> R<()> {", """    fn array_membership(&self, lhs: &str, ph: &str, negated: bool, n: u32, ctx: &mut Ctx) -> R<String> {
        ctx.note("array_param", Tier::Exact, "native array parameter", None)?;
        ctx.bind_array_param(n, ParamBinding::NativeArray);
        Ok(if negated { format!("{lhs} <> ALL({ph})") } else { format!("{lhs} = ANY({ph})") })
    }

    fn binary_op(&self, op: BinaryOp, numeric: bool, ctx: &mut Ctx) -> R<()> {""")
# SQLite: json_each
SL = 'crates/unisql-dialect-sqlite/src/lib.rs'
sub(SL, "use unisql_core::report::{Allowance, Tier, Untranslatable};", "use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};")
sub(SL, "    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {", """    fn array_membership(&self, lhs: &str, ph: &str, negated: bool, n: u32, ctx: &mut Ctx) -> R<String> {
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

    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {""")
# MySQL / MariaDB: JSON_CONTAINS
MY = 'crates/unisql-dialect-mysql/src/lib.rs'
sub(MY, "use unisql_core::report::{Allowance, Tier, Untranslatable};", "use unisql_core::report::{Allowance, ParamBinding, Tier, Untranslatable};")
sub(MY, "    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {", """    fn array_membership(&self, lhs: &str, ph: &str, negated: bool, n: u32, ctx: &mut Ctx) -> R<String> {
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

    fn between(&self, numeric: bool, ctx: &mut Ctx) -> R<()> {""")

# reader: ANY / ALL
RD = 'crates/unisql-sqlparser-adapter/src/lib.rs'
sub(RD, "        E::Exists { subquery, negated } => Expr::Exists", """        E::AnyOp { left, compare_op, right, is_some: _ } => {
            if !matches!(compare_op, sp::BinaryOperator::Eq) {
                return no("any_operator", "only `= ANY(..)` is in the IR");
            }
            quantified(*left, *right, false)?
        }
        E::AllOp { left, compare_op, right } => {
            if !matches!(compare_op, sp::BinaryOperator::NotEq) {
                return no("all_operator", "only `<> ALL(..)` is in the IR");
            }
            quantified(*left, *right, true)?
        }
        E::Exists { subquery, negated } => Expr::Exists""")
sub(RD, "fn like_expr(", """/// `x = ANY(rhs)` / `x <> ALL(rhs)`: an array parameter, an array literal, or a subquery.
fn quantified(left: sp::Expr, right: sp::Expr, negated: bool) -> R<Expr> {
    let lhs = boxed(left)?;
    // `$1::int[]` is only a type hint on the parameter.
    let mut right = right;
    if let sp::Expr::Cast { kind, expr: inner, data_type, format: None } = right.clone() {
        if matches!(kind, sp::CastKind::Cast | sp::CastKind::DoubleColon) && matches!(data_type, sp::DataType::Array(_)) {
            right = *inner;
        }
    }
    match right {
        sp::Expr::Value(v) => match v.value {
            sp::Value::Placeholder(_) => match value(v.value)? {
                Expr::Param(n) => Ok(Expr::AnyParam { expr: lhs, param: n, negated }),
                _ => no("array_param", "expected a numbered parameter"),
            },
            other => no("array_operand", &head(&other)),
        },
        sp::Expr::Array(sp::Array { elem, named: _ }) => Ok(Expr::InList {
            expr: lhs,
            list: elem.into_iter().map(expr).collect::<R<Vec<_>>>()?,
            negated,
        }),
        sp::Expr::Subquery(q) => Ok(Expr::InSubquery { expr: lhs, query: Box::new(query(*q)?), negated }),
        other => no("array_operand", &head(&other)),
    }
}

fn like_expr(""")
print('ok')
