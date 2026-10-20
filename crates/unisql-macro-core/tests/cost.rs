// SPDX-License-Identifier: MIT OR Apache-2.0
//! What a `sql!` call costs at build time (`docs/MACROS.md` section 6). The bounds are loose guards
//! that fail on an accident (a quadratic loop, a re-parse per target), not a benchmark; the figures
//! recorded in the document come from `cargo test -p unisql-macro-core --test cost -- --nocapture`.

use std::time::Instant;

use unisql_macro_core::{Input, expand};

const UPSERT: &str =
    "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v";

fn wide_ddl(tables: usize) -> String {
    (0..tables)
        .map(|n| {
            format!(
                "CREATE TABLE t{n} (id BIGSERIAL PRIMARY KEY, a TEXT NOT NULL, b INT, c TIMESTAMPTZ DEFAULT now(), d JSONB, UNIQUE (a));"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_call_for_four_targets_costs_milliseconds() {
    let input = Input {
        source: "postgres".into(),
        targets: ["postgres", "sqlite", "mysql", "mariadb"]
            .map(String::from)
            .to_vec(),
        allow: vec!["upsert_any_unique_key".into()],
        sql: UPSERT.into(),
        ..Input::default()
    };
    expand(&input).unwrap(); // warm up
    let n = 200;
    let start = Instant::now();
    for _ in 0..n {
        expand(&input).unwrap();
    }
    let per_call = start.elapsed() / n;
    println!("expand, 4 targets, unoptimised: {per_call:?} per call");
    assert!(per_call.as_millis() < 50, "{per_call:?} per call");
}

#[test]
fn reading_a_schema_is_linear_in_its_size() {
    let small = wide_ddl(20);
    let large = wide_ddl(200);
    let time = |ddl: &str| {
        let start = Instant::now();
        let s = unisql::schema_from_ddl(ddl, "postgres").unwrap();
        assert!(s.skipped.is_empty(), "{:?}", s.skipped);
        start.elapsed()
    };
    time(&small); // warm up
    let (t_small, t_large) = (time(&small), time(&large));
    println!("schema_from_ddl, unoptimised: 20 tables {t_small:?}, 200 tables {t_large:?}");
    // ten times the tables must not cost a hundred times the time
    assert!(t_large < t_small * 50, "{t_small:?} vs {t_large:?}");
}
