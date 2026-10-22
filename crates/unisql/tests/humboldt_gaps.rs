// SPDX-License-Identifier: MIT OR Apache-2.0
//! Constructs a real application's PostgreSQL (881 of 883 literals translated) used that the
//! translator refused at the reader: each gets its honest answer per target.

use unisql::{Allowance, Error, Options, translate};

fn all() -> Options {
    Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
}

fn refused(sql: &str, target: &str, leaf: &str) {
    match translate(sql, "postgres", target, &all()) {
        Err(Error::Untranslatable(u)) => assert_eq!(u.construct, leaf, "{target}: {u:?}"),
        other => panic!("{target}: {sql}: {other:?}"),
    }
}

const LATERAL: &str = "SELECT s.id, latest.zone_id FROM students s \
    LEFT JOIN LATERAL (SELECT n.zone_id FROM pickup_notices n WHERE n.student_id = s.id \
    ORDER BY n.created_at DESC LIMIT 1) latest ON true WHERE s.school_id = $1";

#[test]
fn left_join_lateral_keeps_lateral_where_the_target_has_it() {
    for target in ["postgres", "mysql"] {
        let t = translate(LATERAL, "postgres", target, &all())
            .unwrap_or_else(|e| panic!("{target}: {e:?}"));
        assert!(t.sql.contains("LEFT JOIN LATERAL ("), "{target}: {}", t.sql);
    }
}

#[test]
fn lateral_is_refused_where_the_target_cannot_refer_leftwards() {
    for target in ["sqlite", "mariadb"] {
        refused(LATERAL, target, "lateral");
    }
}
