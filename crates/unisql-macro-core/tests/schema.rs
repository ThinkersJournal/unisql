// SPDX-License-Identifier: MIT OR Apache-2.0
//! The checks that need the caller's tables, and the `Checker` seam. Each failing call has a passing twin.

use unisql::ast::Statement;
use unisql_macro_core::{CheckContext, Checker, Input, Kind, Problem, expand, expand_with};

const DDL: &str = "CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT, n BIGINT NOT NULL);
                   CREATE TABLE skipped_view_source (id INT);";

fn call(sql: &str) -> Input {
    Input {
        source: "postgres".into(),
        targets: vec!["sqlite".into()],
        sql: sql.into(),
        schema: Some(unisql::schema_from_ddl(DDL, "postgres").unwrap().schema),
        ..Input::default()
    }
}

fn messages(input: &Input) -> Vec<String> {
    expand(input)
        .expect_err("expected a failure")
        .into_iter()
        .inspect(|p| assert_eq!(p.kind, Kind::Catalog))
        .map(|p| p.message)
        .collect()
}

#[test]
fn a_row_of_the_wrong_width() {
    let m = messages(&call("INSERT INTO kv (k, v) VALUES ($1)"));
    assert_eq!(m.len(), 1, "{m:?}");
    assert!(m[0].contains("VALUES row 1 has 1 value(s)"), "{m:?}");
    assert!(expand(&call("INSERT INTO kv (k, v) VALUES ($1, $2)")).is_ok());
}

#[test]
fn every_wrong_row_is_reported_not_the_first() {
    let m = messages(&call("INSERT INTO kv (k, v) VALUES ($1), ($2, $3), ($4)"));
    assert_eq!(m.len(), 2, "{m:?}");
    assert!(m[0].contains("row 1") && m[1].contains("row 3"), "{m:?}");
}

#[test]
fn without_a_column_list_the_width_is_the_tables() {
    assert_eq!(messages(&call("INSERT INTO kv VALUES ($1, $2)")).len(), 1);
    assert!(expand(&call("INSERT INTO kv VALUES ($1, $2, $3)")).is_ok());
}

#[test]
fn a_column_the_table_lacks_and_a_column_listed_twice() {
    let m = messages(&call("INSERT INTO kv (k, nope) VALUES ($1, $2)"));
    assert_eq!(m, ["table `kv` has no column `nope`"]);
    let m = messages(&call("INSERT INTO kv (k, K) VALUES ($1, $2)"));
    assert!(m[0].contains("listed twice"), "{m:?}");
    // the same name in another case is not reported as missing
    assert!(expand(&call("INSERT INTO kv (K, V) VALUES ($1, $2)")).is_ok());
}

#[test]
fn update_set_names_a_column_the_table_lacks() {
    let m = messages(&call("UPDATE kv SET nope = $1 WHERE k = $2"));
    assert_eq!(m, ["table `kv` has no column `nope`"]);
    assert!(expand(&call("UPDATE kv SET v = $1 WHERE n = 1")).is_ok());
}

#[test]
fn what_the_schema_does_not_state_is_not_reported() {
    // a table the schema does not list: unknown, never wrong
    assert!(expand(&call("INSERT INTO elsewhere (a, b) VALUES ($1, $2)")).is_ok());
    // no schema at all: a missing column cannot be known ...
    let mut c = call("INSERT INTO kv (k, nope) VALUES ($1, $2)");
    c.schema = None;
    assert!(expand(&c).is_ok());
    // ... but a row that does not match its own column list is wrong from the statement alone
    c.sql = "INSERT INTO kv (k, v) VALUES ($1)".into();
    assert_eq!(expand(&c).unwrap_err()[0].kind, Kind::Catalog);
}

#[test]
fn a_schema_also_feeds_the_translator() {
    // `n` is BIGINT, so comparing it needs no `text_collation` on MySQL
    let mut c = call("SELECT k FROM kv WHERE n < $1");
    c.targets = vec!["mysql".into()];
    assert!(expand(&c).is_ok());
    c.schema = None;
    assert_eq!(expand(&c).unwrap_err()[0].kind, Kind::Refused);
}

struct NoDeletes;

impl Checker for NoDeletes {
    fn check(&self, stmt: &Statement, ctx: &CheckContext<'_>) -> Vec<Problem> {
        match stmt {
            Statement::Delete(_) => vec![Problem {
                kind: Kind::Catalog,
                message: format!("no DELETE in {} source", ctx.source),
            }],
            _ => vec![],
        }
    }
}

#[test]
fn a_checker_of_the_callers_runs_after_syntax_and_before_translation() {
    let checkers: &[&dyn Checker] = &[&NoDeletes];
    let err = expand_with(&call("DELETE FROM kv WHERE k = $1"), checkers).unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].message, "no DELETE in postgres source");
    assert!(expand_with(&call("SELECT k FROM kv WHERE n = 1"), checkers).is_ok());
    // syntax comes first: a checker is never handed text that does not parse
    let err = expand_with(&call("DELET FROM kv"), checkers).unwrap_err();
    assert_eq!(err[0].kind, Kind::Syntax);
}

#[test]
fn a_literal_null_for_a_not_null_column() {
    let m = messages(&call("INSERT INTO kv (k, v, n) VALUES ($1, $2, NULL)"));
    assert_eq!(
        m,
        ["column `n` of `kv` is NOT NULL, but the INSERT gives it NULL"]
    );
    // a nullable column takes NULL; the twin gives the NOT NULL column a value
    assert!(expand(&call("INSERT INTO kv (k, v, n) VALUES ($1, NULL, $2)")).is_ok());
    // without a column list the order is not stated, so nothing is claimed
    assert!(expand(&call("INSERT INTO kv VALUES ($1, $2, NULL)")).is_ok());
    // every row is looked at
    let m = messages(&call("INSERT INTO kv (k, n) VALUES ($1, $2), (NULL, $3)"));
    assert_eq!(
        m,
        ["column `k` of `kv` is NOT NULL, but the INSERT gives it NULL"]
    );
}

#[test]
fn update_set_to_null_for_a_not_null_column() {
    let m = messages(&call("UPDATE kv SET n = NULL WHERE k = $1"));
    assert_eq!(
        m,
        ["column `n` of `kv` is NOT NULL, but the UPDATE sets it to NULL"]
    );
    assert!(expand(&call("UPDATE kv SET v = NULL WHERE k = $1")).is_ok());
}

#[test]
fn a_column_that_takes_a_value_for_null_is_not_reported() {
    // A SQLite `INTEGER PRIMARY KEY` is the rowid: NULL (or leaving it out) assigns the next one.
    let ddl = "CREATE TABLE r (id INTEGER PRIMARY KEY, x TEXT NOT NULL);";
    let mut input = call("INSERT INTO r (id, x) VALUES (NULL, $1)");
    input.schema = Some(unisql::schema_from_ddl(ddl, "sqlite").unwrap().schema);
    assert!(expand(&input).is_ok());
    // an unknown flag is no claim: a hand-built column without nullability is never reported
    let hand = unisql::Schema::new()
        .with_column("h", "a", unisql::Column::text())
        .with_column("h", "b", unisql::Column::text());
    let mut input = call("INSERT INTO h (a, b) VALUES ($1, NULL)");
    input.schema = Some(hand);
    assert!(expand(&input).is_ok());
}
