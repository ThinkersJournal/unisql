// SPDX-License-Identifier: MIT OR Apache-2.0
//! Defects the MySQL-source corpus (`unisql-differential/corpus/mysql`) found: each statement was
//! translated to text that meant something else, or that the target engine rejects.

use unisql::{Allowance, Error, Options, ReadError, translate};

fn all() -> Options {
    Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
}

fn not_in_ir(sql: &str, source: &str, want: &str) {
    match translate(sql, source, "postgres", &all()) {
        Err(Error::Read(ReadError::NotInIr { construct, .. })) => assert_eq!(construct, want),
        other => panic!("{sql}: {other:?}"),
    }
}

#[test]
fn update_low_priority_is_not_read_as_a_table_named_low_priority() {
    not_in_ir(
        "UPDATE LOW_PRIORITY users SET score = 0 WHERE id = ?",
        "mysql",
        "update_option",
    );
    not_in_ir(
        "UPDATE IGNORE users SET score = 0 WHERE id = ?",
        "mysql",
        "update_option",
    );
    // An ordinary UPDATE still reads.
    let t = translate(
        "UPDATE users SET score = 0 WHERE id = ?",
        "mysql",
        "postgres",
        &all(),
    );
    assert!(t.is_ok(), "{t:?}");
}

#[test]
fn default_in_values_is_not_a_column_named_default() {
    for (source, sql) in [
        ("mysql", "INSERT INTO kv (k, v) VALUES (?, DEFAULT)"),
        ("postgres", "INSERT INTO kv (k, v) VALUES ($1, DEFAULT)"),
    ] {
        not_in_ir(sql, source, "default_value");
    }
}

#[test]
fn sqlite_group_concat_distinct_takes_no_separator() {
    let ok = translate(
        "SELECT GROUP_CONCAT(DISTINCT status) FROM orders",
        "mysql",
        "sqlite",
        &all(),
    )
    .unwrap();
    assert_eq!(
        ok.sql,
        r#"SELECT GROUP_CONCAT(DISTINCT "status") FROM "orders""#
    );
    let refused = translate(
        "SELECT GROUP_CONCAT(DISTINCT status SEPARATOR ';') FROM orders",
        "mysql",
        "sqlite",
        &all(),
    );
    assert!(
        matches!(&refused, Err(Error::Untranslatable(u)) if u.construct == "fn:string_agg"),
        "{refused:?}"
    );
}
