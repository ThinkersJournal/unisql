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

const CROSS_LATERAL: &str = "SELECT s.id, t.zone_id FROM students s \
    CROSS JOIN LATERAL (SELECT n.zone_id FROM pickup_notices n WHERE n.student_id = s.id \
    ORDER BY n.created_at DESC LIMIT 1) t";

#[test]
fn cross_join_lateral_follows_the_same_per_target_answer() {
    for target in ["postgres", "mysql"] {
        let t = translate(CROSS_LATERAL, "postgres", target, &all())
            .unwrap_or_else(|e| panic!("{target}: {e:?}"));
        assert!(
            t.sql.contains("CROSS JOIN LATERAL ("),
            "{target}: {}",
            t.sql
        );
    }
    for target in ["sqlite", "mariadb"] {
        refused(CROSS_LATERAL, target, "lateral");
    }
}

const REGCLASS: &str = "SELECT nextval('students_id_seq'::regclass), $1::regclass";

#[test]
fn regclass_cast_stays_on_postgres() {
    let t = translate(REGCLASS, "postgres", "postgres", &all()).unwrap();
    assert!(
        t.sql.contains("::regclass") || t.sql.contains("REGCLASS"),
        "{}",
        t.sql
    );
}

#[test]
fn regclass_cast_is_refused_where_there_is_no_catalog() {
    for target in ["sqlite", "mysql", "mariadb"] {
        refused("SELECT 'students'::regclass", target, "cast.to_regclass");
    }
}
