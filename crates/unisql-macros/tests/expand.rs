// SPDX-License-Identifier: MIT OR Apache-2.0
//! What `sql!` embeds equals what `unisql::translate` returns, and the value is usable in `const`.

use unisql::{Allowance, Options};
use unisql_embed::{Binding, Query, Target};
use unisql_macros::sql;

const UPSERT: &str =
    "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v";

const Q: Query = sql!(
    source = postgres,
    targets = [sqlite, mysql, mariadb, postgres],
    allow = [upsert_any_unique_key],
    requires = ["upsert"],
    forbids = ["fn:md5"],
    "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v"
);

const SQLITE: &str = match Q.for_target(Target::Sqlite) {
    Some(r) => r.sql,
    None => "",
};

#[test]
fn embedded_text_equals_translate_for_every_target() {
    let opts = Options::default().with(Allowance::UpsertAnyUniqueKey);
    assert_eq!(Q.params, 2);
    assert_eq!(Q.rendered.len(), 4);
    for r in Q.rendered {
        let t = unisql::translate(UPSERT, "postgres", r.target.name(), &opts).unwrap();
        assert_eq!(r.sql, t.sql, "{:?}", r.target);
        assert_eq!(r.param_order, t.param_order.as_slice());
        assert_eq!(r.param_bindings.len(), t.param_bindings.len());
        assert!(r.param_bindings.iter().all(|b| *b == Binding::Scalar));
    }
    assert!(Q.uses("upsert"));
    assert!(!Q.uses("fn:md5"));
}

#[test]
fn the_value_is_a_const() {
    assert!(SQLITE.contains("ON CONFLICT"), "{SQLITE}");
}

#[test]
fn an_unnamed_target_is_absent() {
    const ONE: Query = sql!(source = postgres, targets = [sqlite], "SELECT 1");
    assert!(ONE.for_target(Target::Sqlite).is_some());
    assert!(ONE.for_target(Target::Mysql).is_none());
}

#[test]
fn the_crate_argument_redirects_the_expansion_path() {
    mod mine {
        pub use unisql_embed::*;
    }
    const R: mine::Query = sql!(
        crate = mine,
        source = postgres,
        targets = [sqlite],
        "SELECT 1"
    );
    assert_eq!(R.params, 0);
}

#[test]
fn arguments_come_back_as_a_tuple_after_the_count_is_checked() {
    let (q, (a, b)) = sql!(
        source = postgres,
        targets = [sqlite],
        allow = [text_collation],
        "SELECT a FROM t WHERE x = $1 AND y = $2",
        7_i64,
        "name"
    );
    assert_eq!((q.params, a, b), (2, 7_i64, "name"));
    // no arguments given: the bare Query, as before
    const BARE: Query = sql!(source = postgres, targets = [sqlite], "SELECT 1");
    assert_eq!(BARE.params, 0);
    // a trailing comma with no arguments is still the bare Query
    let bare: Query = sql!(source = postgres, targets = [sqlite], "SELECT 1",);
    assert_eq!(bare.params, 0);
}

// `schema = ".."` is relative to this crate's manifest directory and read at build time.
#[test]
fn a_schema_file_feeds_the_checks_and_the_translator() {
    // `n` is BIGINT in tests/schema.sql: no `text_collation` is asked for on MySQL
    const Q: Query = sql!(
        source = postgres,
        targets = [mysql],
        schema = "tests/schema.sql",
        "SELECT k FROM kv WHERE n < $1"
    );
    assert_eq!(Q.params, 1);
}

#[test]
fn an_inline_schema_works_the_same() {
    const Q: Query = sql!(
        source = postgres,
        targets = [mysql],
        schema_ddl = "CREATE TABLE kv (k TEXT PRIMARY KEY, n BIGINT NOT NULL);",
        "SELECT k FROM kv WHERE n < $1"
    );
    assert_eq!(Q.params, 1);
}

#[test]
fn a_json_schema_is_read_as_a_snapshot() {
    // `n` is BIGINT in tests/schema.json: no `text_collation` is asked for on MySQL
    const Q: Query = sql!(
        source = postgres,
        targets = [mysql],
        schema = "tests/schema.json",
        "SELECT k FROM kv WHERE n < $1"
    );
    assert_eq!(Q.params, 1);
}
