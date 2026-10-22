// SPDX-License-Identifier: MIT OR Apache-2.0
//! IR-to-IR rewrites that targets opt into. Each is a pure function with a stated precondition and
//! returns `Err(reason)` when it cannot be applied *correctly* (the target then refuses).

use crate::ast::*;

/// `SELECT DISTINCT ON (k..) proj FROM .. ORDER BY o..` ->
///
/// ```sql
/// SELECT __u0 AS a, __u1 AS b FROM (
///   SELECT p0 AS __u0, p1 AS __u1, o0 AS __o0, ROW_NUMBER() OVER (PARTITION BY k.. ORDER BY o..) AS __rn
///   FROM .. WHERE ..) AS __d
/// WHERE __rn = 1 ORDER BY __o0 ..
/// ```
///
/// Differences from PostgreSQL, stated: without an `ORDER BY`, which row survives per key is
/// arbitrary on both engines (not the same row); a `*` projection cannot be emulated because the
/// helper columns would appear in the result.
/// Why a rewrite cannot be applied correctly: the leaf id of the form that is refused, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cannot {
    pub leaf: &'static str,
    pub why: String,
}

impl Cannot {
    fn new(leaf: &'static str, why: &str) -> Self {
        Cannot {
            leaf,
            why: why.to_string(),
        }
    }
}

pub fn distinct_on_via_window(q: &Query) -> Result<Query, Cannot> {
    let SetExpr::Select(sel) = &q.body else {
        return Err(Cannot::new(
            "distinct_on.set_operation",
            "DISTINCT ON on a set operation",
        ));
    };
    let Distinct::On(keys) = &sel.distinct else {
        return Err(Cannot::new("distinct_on", "not a DISTINCT ON select"));
    };
    if sel.lock.is_some() {
        return Err(Cannot::new(
            "distinct_on.row_lock",
            "DISTINCT ON with a locking clause",
        ));
    }

    let mut inner_proj = Vec::new();
    let mut outer_proj = Vec::new();
    for (i, item) in sel.projection.iter().enumerate() {
        match item {
            SelectItem::Wildcard | SelectItem::QualifiedWildcard(_) => {
                return Err(Cannot::new(
                    "distinct_on.star_projection",
                    "DISTINCT ON with a * projection cannot be emulated; list the columns explicitly",
                ));
            }
            SelectItem::Expr { expr, alias } => {
                let hidden = format!("__u{i}");
                let shown = match (alias, expr) {
                    (Some(a), _) => a.clone(),
                    (None, Expr::Column(n)) => n.last().to_string(),
                    (None, Expr::Function(f)) => f.name.clone(),
                    _ => "?column?".to_string(),
                };
                inner_proj.push(SelectItem::Expr {
                    expr: expr.clone(),
                    alias: Some(hidden.clone()),
                });
                outer_proj.push(SelectItem::Expr {
                    expr: Expr::Column(Name::simple(hidden)),
                    alias: Some(shown),
                });
            }
        }
    }

    let mut outer_order = Vec::new();
    for (j, o) in q.order_by.iter().enumerate() {
        let hidden = format!("__o{j}");
        inner_proj.push(SelectItem::Expr {
            expr: o.expr.clone(),
            alias: Some(hidden.clone()),
        });
        outer_order.push(OrderItem {
            expr: Expr::Column(Name::simple(hidden)),
            desc: o.desc,
            nulls_first: o.nulls_first,
        });
    }

    inner_proj.push(SelectItem::Expr {
        expr: Expr::Function(Function {
            name: "row_number".into(),
            args: vec![],
            distinct: false,
            star: false,
            filter: None,
            order_by: vec![],
            over: Some(Window {
                partition_by: keys.clone(),
                order_by: q.order_by.clone(),
                frame: None,
            }),
        }),
        alias: Some("__rn".into()),
    });

    let inner = Query {
        with: vec![],
        recursive: false,
        body: SetExpr::Select(Box::new(Select {
            distinct: Distinct::No,
            projection: inner_proj,
            from: sel.from.clone(),
            where_: sel.where_.clone(),
            group_by: sel.group_by.clone(),
            group_by_rollup: sel.group_by_rollup,
            having: sel.having.clone(),
            lock: None,
        })),
        order_by: vec![],
        limit: None,
        offset: None,
    };

    let outer = Select {
        distinct: Distinct::No,
        projection: outer_proj,
        from: vec![TableRef::Subquery {
            lateral: false,
            query: Box::new(inner),
            alias: "__d".into(),
        }],
        where_: Some(Expr::Binary {
            op: BinaryOp::Eq,
            left: Box::new(Expr::Column(Name::simple("__rn"))),
            right: Box::new(Expr::Literal(Literal::Number("1".into()))),
        }),
        group_by: vec![],
        group_by_rollup: false,
        having: None,
        lock: None,
    };
    Ok(Query {
        with: q.with.clone(),
        recursive: q.recursive,
        body: SetExpr::Select(Box::new(outer)),
        order_by: outer_order,
        limit: q.limit.clone(),
        offset: q.offset.clone(),
    })
}
