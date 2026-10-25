// SPDX-License-Identifier: MIT OR Apache-2.0
//! The prepare check (DESIGN.md section 6.2) as a reusable helper: a program's statement catalogue is
//! translated for each target and `PREPARE`d on that target's engine. This is also the recipe a consumer
//! copies: open the engines with the target schema, call `prepare_check`, assert every row is `Prepared`.

use unisql::Options;
use unisql_differential::{Engines, PrepareOutcome, prepare_check};

const PG: &str = "CREATE TABLE kv (k text PRIMARY KEY, v text, n integer);";
const LITE: &str = "CREATE TABLE kv (k text PRIMARY KEY, v text, n integer);";

fn open() -> Engines {
    Engines::open(PG, LITE)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"))
}

#[test]
fn a_catalogue_is_translated_and_prepared_on_each_target() {
    let mut e = open();
    let catalogue = [
        ("get", "SELECT v FROM kv WHERE k = $1"),
        (
            "put",
            "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v",
        ),
    ];
    let rows = prepare_check(
        &mut e,
        "postgres",
        &["postgres", "sqlite"],
        &Options::default(),
        &catalogue,
    );
    assert_eq!(rows.len(), 4, "one row per statement and target");
    for r in &rows {
        assert_eq!(
            r.outcome,
            PrepareOutcome::Prepared,
            "{} on {}",
            r.id,
            r.target
        );
    }
}

#[test]
fn a_statement_the_target_engine_rejects_is_reported_with_its_text() {
    // control for the check itself: the same text, prepared directly, fails on the engine
    let mut e = open();
    assert!(e.prepares("sqlite", "SELECT v FROM missing_table").is_err());
    let rows = prepare_check(
        &mut e,
        "postgres",
        &["sqlite"],
        &Options::default(),
        &[("bad", "SELECT v FROM missing_table")],
    );
    match &rows[0].outcome {
        PrepareOutcome::Rejected { sql, error } => {
            assert!(sql.contains("missing_table"), "{sql}");
            assert!(error.contains("missing_table"), "{error}");
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[test]
fn a_refused_statement_is_reported_not_prepared() {
    let mut e = open();
    let rows = prepare_check(
        &mut e,
        "postgres",
        &["sqlite"],
        &Options::default(),
        &[("age", "SELECT age(now(), now())")],
    );
    assert!(
        matches!(rows[0].outcome, PrepareOutcome::Refused(_)),
        "{:?}",
        rows[0].outcome
    );
}
