// SPDX-License-Identifier: MIT OR Apache-2.0
//! `age(a, b)` stays on PostgreSQL and is refused with a reason elsewhere.

use unisql::{Error, Options, translate};

const AGE: &str = "SELECT age(updated_at, created_at) FROM users";

#[test]
fn age_passes_through_on_postgres() {
    let t = translate(AGE, "postgres", "postgres", &Options::default()).unwrap();
    assert!(t.sql.to_lowercase().contains("age("), "{}", t.sql);
}

#[test]
fn age_is_refused_where_there_is_no_interval() {
    for target in ["sqlite", "mysql", "mariadb"] {
        match translate(AGE, "postgres", target, &Options::default()) {
            Err(Error::Untranslatable(u)) => assert_eq!(u.construct, "fn:age", "{target}"),
            other => panic!("{target}: {other:?}"),
        }
    }
}
