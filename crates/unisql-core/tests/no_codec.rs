// SPDX-License-Identifier: MIT OR Apache-2.0
//! A target that does not implement `array_membership` must refuse an array parameter loudly: no
//! text, a named construct, and a reason. (The default is the safe one; nothing is passed through.)

use unisql_core::ast::*;
use unisql_core::{Options, Target, write};

struct Bare;

impl Target for Bare {
    fn name(&self) -> &'static str {
        "bare"
    }
}

fn any_param_query(negated: bool) -> Statement {
    Statement::Query(Box::new(Query {
        with: vec![],
        recursive: false,
        body: SetExpr::Select(Box::new(Select {
            distinct: Distinct::No,
            projection: vec![SelectItem::Wildcard],
            from: vec![TableRef::Table {
                name: Name::simple("t"),
                alias: None,
            }],
            where_: Some(Expr::AnyParam {
                expr: Box::new(Expr::Column(Name::simple("id"))),
                param: 1,
                negated,
            }),
            group_by: vec![],
            group_by_rollup: false,
            having: None,
            lock: None,
        })),
        order_by: vec![],
        limit: None,
        offset: None,
    }))
}

#[test]
fn array_param_without_a_codec_is_refused() {
    for negated in [false, true] {
        let err = write(
            &any_param_query(negated),
            "postgres",
            &Bare,
            &Options::default(),
        )
        .unwrap_err();
        assert_eq!(err.construct, "array_param");
        assert_eq!(err.subject(), "array_param");
        assert!(err.reason.contains("no codec"), "{}", err.reason);
        assert!(!err.alternatives.is_empty());
    }
}

#[test]
fn a_param_used_as_array_and_scalar_is_refused() {
    struct Codec;
    impl Target for Codec {
        fn name(&self) -> &'static str {
            "codec"
        }
        fn array_membership(
            &self,
            lhs: &str,
            ph: &str,
            _neg: bool,
            n: u32,
            ctx: &mut unisql_core::Ctx,
        ) -> Result<String, unisql_core::Untranslatable> {
            ctx.bind_array_param(n, unisql_core::ParamBinding::JsonArrayText);
            Ok(format!("{lhs} IN {ph}"))
        }
    }
    // WHERE id = ANY($1) AND id = $1
    let Statement::Query(mut q) = any_param_query(false) else {
        unreachable!()
    };
    let SetExpr::Select(s) = &mut q.body else {
        unreachable!()
    };
    let any = s.where_.take().unwrap();
    s.where_ = Some(Expr::Binary {
        op: BinaryOp::And,
        left: Box::new(any),
        right: Box::new(Expr::Binary {
            op: BinaryOp::Eq,
            left: Box::new(Expr::Column(Name::simple("id"))),
            right: Box::new(Expr::Param(1)),
        }),
    });
    let err = write(
        &Statement::Query(q),
        "postgres",
        &Codec,
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(err.construct, "array_param");
    assert!(
        err.reason
            .contains("both as an array and as a single value"),
        "{}",
        err.reason
    );
}
