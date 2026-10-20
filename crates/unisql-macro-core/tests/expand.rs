// SPDX-License-Identifier: MIT OR Apache-2.0
//! One failing call per check, each with a passing twin (the same call with the fault removed).

use unisql_macro_core::{Input, Kind, expand};

const UPSERT: &str =
    "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v";

fn call(sql: &str, targets: &[&str]) -> Input {
    Input {
        source: "postgres".into(),
        targets: targets.iter().map(|t| t.to_string()).collect(),
        sql: sql.into(),
        ..Input::default()
    }
}

fn kinds(input: &Input) -> Vec<Kind> {
    expand(input)
        .expect_err("expected a failure")
        .iter()
        .map(|p| p.kind)
        .collect()
}

#[test]
fn a_clean_call_embeds_the_same_text_translate_gives() {
    let mut c = call(UPSERT, &["sqlite", "mysql"]);
    c.allow = vec!["upsert_any_unique_key".into()];
    let e = expand(&c).unwrap();
    assert_eq!(e.params, 2);
    assert_eq!(
        e.rendered
            .iter()
            .map(|r| r.target.as_str())
            .collect::<Vec<_>>(),
        ["sqlite", "mysql"]
    );
    for r in &e.rendered {
        let opts = unisql::Options::default().with(unisql::Allowance::UpsertAnyUniqueKey);
        let t = unisql::translate(UPSERT, "postgres", &r.target, &opts).unwrap();
        assert_eq!(
            (&r.sql, &r.param_order, &r.param_bindings),
            (&t.sql, &t.param_order, &t.param_bindings)
        );
    }
    assert!(e.constructs.contains(&"upsert".to_string()));
    let mut sorted = e.constructs.clone();
    sorted.sort();
    assert_eq!(e.constructs, sorted);
}

#[test]
fn syntax_error() {
    assert_eq!(kinds(&call("SELEC 1", &["sqlite"])), [Kind::Syntax]);
    assert!(expand(&call("SELECT 1", &["sqlite"])).is_ok());
}

#[test]
fn skipped_placeholder() {
    // `text_collation` is granted so that the skipped `$2` is the only fault
    let mut bad = call(
        "SELECT a FROM t WHERE x = $1 AND y = $3",
        &["sqlite", "mysql"],
    );
    bad.allow = vec!["text_collation".into()];
    assert_eq!(kinds(&bad), [Kind::Placeholders]);
    let mut ok = call(
        "SELECT a FROM t WHERE x = $1 AND y = $2",
        &["sqlite", "mysql"],
    );
    ok.allow = vec!["text_collation".into()];
    assert_eq!(expand(&ok).unwrap().params, 2);
    // a parameter used twice is one parameter, not a gap
    let twice = call("SELECT a FROM t WHERE x = $1 OR y = $1", &["sqlite"]);
    let e = expand(&twice).unwrap();
    assert_eq!(
        (e.params, e.rendered[0].param_order.clone()),
        (1, vec![1, 1])
    );
}

#[test]
fn no_targets_unknown_and_duplicate_targets() {
    assert_eq!(kinds(&call("SELECT 1", &[])), [Kind::Targets]);
    assert_eq!(kinds(&call("SELECT 1", &["oracle"])), [Kind::Targets]);
    assert_eq!(
        kinds(&call("SELECT 1", &["sqlite", "sqlite"])),
        [Kind::Targets]
    );
    assert!(expand(&call("SELECT 1", &["sqlite", "mariadb"])).is_ok());
}

#[test]
fn unknown_source() {
    let mut c = call("SELECT 1", &["sqlite"]);
    c.source = "oracle".into();
    assert_eq!(kinds(&c), [Kind::Source]);
}

#[test]
fn a_refusal_names_the_target_and_each_refusing_target_is_reported() {
    // md5 has no SQLite mapping; PostgreSQL has one
    let c = call("SELECT md5($1)", &["postgres", "sqlite"]);
    let errs = expand(&c).unwrap_err();
    assert_eq!(
        errs.iter().map(|p| p.kind).collect::<Vec<_>>(),
        [Kind::Refused]
    );
    assert!(errs[0].message.contains("sqlite"), "{}", errs[0].message);
    assert!(expand(&call("SELECT md5($1)", &["postgres"])).is_ok());
}

#[test]
fn a_conditional_needs_its_allowance_named() {
    // text comparison on an unknown column needs `text_collation` on MySQL
    let sql = "SELECT a FROM t WHERE name < $1";
    let denied = expand(&call(sql, &["mysql"])).unwrap_err();
    assert_eq!(denied[0].kind, Kind::Refused);
    assert!(
        denied[0].message.contains("text_collation"),
        "{}",
        denied[0].message
    );
    let mut granted = call(sql, &["mysql"]);
    granted.allow = vec!["text_collation".into()];
    assert!(expand(&granted).is_ok());
}

#[test]
fn unknown_allowance_lists_the_known_ones() {
    let mut c = call("SELECT 1", &["sqlite"]);
    c.allow = vec!["text_collatoin".into()];
    let errs = expand(&c).unwrap_err();
    assert_eq!(errs[0].kind, Kind::Allow);
    assert!(errs[0].message.contains("text_collation"));
}

#[test]
fn requires_and_forbids_check_the_construct_ids() {
    let mut c = call(UPSERT, &["sqlite"]);
    c.requires = vec!["upsert".into()];
    assert!(expand(&c).is_ok());
    c.requires = vec!["returning".into()];
    assert_eq!(kinds(&c), [Kind::Requires]);
    c.requires = vec![];
    c.forbids = vec!["upsert".into()];
    assert_eq!(kinds(&c), [Kind::Forbids]);
    c.forbids = vec!["returning".into()];
    assert!(expand(&c).is_ok());
}
