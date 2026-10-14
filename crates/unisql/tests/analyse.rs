// SPDX-License-Identifier: MIT OR Apache-2.0
//! `analyse` is target-free: it reports what a statement needs, and which of its forms some target
//! refuses. No engine is involved.

use unisql::{Allowance, Column, Options, Schema, analyse, analyse_with, translate};

fn all() -> Options {
    Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
}

#[test]
fn a_plain_cast_needs_its_leaf_and_nothing_refused() {
    let a = analyse("SELECT CAST(id AS TEXT) FROM users", "postgres").unwrap();
    assert!(a.constructs.contains("cast.to_text"), "{:?}", a.constructs);
    assert!(!a.refused.contains("cast.to_text"), "{:?}", a.refused);
    assert_eq!(a.unreadable, None);
}

#[test]
fn a_cast_to_uuid_names_its_own_leaf_as_refused() {
    let a = analyse("SELECT CAST(name AS UUID) FROM users", "postgres").unwrap();
    assert!(a.constructs.contains("cast.to_uuid"), "{:?}", a.constructs);
    assert!(a.refused.contains("cast.to_uuid"), "{:?}", a.refused);
}

#[test]
fn forms_refused_by_some_target_are_listed() {
    let lock = analyse("SELECT id FROM users WHERE id = 1 FOR UPDATE", "postgres").unwrap();
    assert!(lock.constructs.contains("row_lock"));
    assert!(lock.refused.contains("row_lock"), "{:?}", lock.refused);

    let md5 = analyse("SELECT md5(name) FROM users", "postgres").unwrap();
    assert!(md5.refused.contains("fn:md5"), "{:?}", md5.refused);

    let star = analyse(
        "SELECT DISTINCT ON (city) * FROM users ORDER BY city",
        "postgres",
    )
    .unwrap();
    assert!(
        star.refused.contains("distinct_on.star_projection"),
        "{:?}",
        star.refused
    );
}

#[test]
fn a_statement_outside_the_ir_is_unreadable_not_an_error() {
    let a = analyse(
        "SELECT row_number() OVER w FROM users WINDOW w AS (ORDER BY id)",
        "postgres",
    )
    .unwrap();
    assert!(a.unreadable.is_some());
    assert!(a.constructs.is_empty());
}

#[test]
fn syntax_errors_are_errors() {
    assert!(analyse("SELEKT 1", "postgres").is_err());
    assert!(analyse("SELECT 1", "klingon").is_err());
}

#[test]
fn source_dialect_constructs_are_normalised() {
    let a = analyse(
        "SELECT ifnull(city, 'x'), group_concat(name) FROM users",
        "sqlite",
    )
    .unwrap();
    assert!(a.constructs.contains("fn:coalesce"), "{:?}", a.constructs);
    assert!(a.constructs.contains("fn:string_agg"));
    let m = analyse(
        "SELECT IF(age > 30, 'a', 'b') FROM users LIMIT 2, 3",
        "mysql",
    )
    .unwrap();
    assert!(
        m.constructs.contains("limit") && m.constructs.contains("offset"),
        "{:?}",
        m.constructs
    );
}

#[test]
fn interval_literals_are_the_make_interval_form() {
    // PostgreSQL spelling, MySQL spelling and the explicit call are one statement to every target.
    let pg = analyse(
        "SELECT 1 FROM e WHERE at > now() - interval '5 minutes'",
        "postgres",
    )
    .unwrap();
    let my = analyse(
        "SELECT 1 FROM e WHERE at > NOW() - INTERVAL 5 MINUTE",
        "mysql",
    )
    .unwrap();
    let call = analyse(
        "SELECT 1 FROM e WHERE at > now() - make_interval(mins => 5)",
        "postgres",
    )
    .unwrap();
    for a in [&pg, &my, &call] {
        assert!(a.unreadable.is_none(), "{:?}", a.unreadable);
        assert!(
            a.constructs.contains("interval_arithmetic"),
            "{:?}",
            a.constructs
        );
        assert!(
            a.constructs.contains("fn:make_interval"),
            "{:?}",
            a.constructs
        );
    }
    let t = translate(
        "SELECT 1 FROM e WHERE at > NOW() - INTERVAL 5 MINUTE",
        "mysql",
        "postgres",
        &all(),
    )
    .unwrap();
    let flat = t.sql.to_lowercase().replace('"', "");
    assert!(flat.contains("make_interval(mins => 5)"), "{}", t.sql);
    // months are read and then refused by the targets, not guessed
    let month = analyse("SELECT now() - interval '1 month'", "postgres").unwrap();
    assert!(
        month.refused.contains("fn:make_interval.month_year"),
        "{:?}",
        month.refused
    );
    // compound intervals are not in the IR
    let compound = analyse("SELECT now() - interval '1 day 2 hours'", "postgres").unwrap();
    assert!(compound.unreadable.is_some());
}

#[test]
fn a_data_modifying_cte_is_read_and_named() {
    let sql = "WITH s AS (INSERT INTO schools (name) VALUES ('x') RETURNING id) \
               INSERT INTO links (school_id) SELECT id FROM s";
    let a = analyse(sql, "postgres").unwrap();
    assert!(a.unreadable.is_none(), "{:?}", a.unreadable);
    for leaf in ["with.insert", "cte.data_modifying", "returning"] {
        assert!(a.constructs.contains(leaf), "{leaf}: {:?}", a.constructs);
    }
    // Only PostgreSQL has it: every other target refuses the CTE (or, first, the WITH).
    assert!(a.refused.contains("cte.data_modifying") || a.refused.contains("with.insert"));
    // ... and PostgreSQL writes it back.
    let t = translate(sql, "postgres", "postgres", &all()).unwrap();
    assert!(t.sql.to_uppercase().starts_with("WITH "), "{}", t.sql);
}

fn users() -> Schema {
    Schema::new()
        .with_column("users", "id", Column::numeric())
        .with_column("users", "name", Column::text())
        .with_column("users", "age", Column::numeric())
        .with_column("orders", "user_id", Column::numeric())
        .with_column("orders", "status", Column::text())
}

#[test]
fn a_schema_removes_text_comparison_for_non_text_columns_only() {
    let opts = Options::default().with_schema(users());
    let sql = |q: &str| analyse_with(q, "postgres", &opts).unwrap().constructs;
    let plain = |q: &str| analyse(q, "postgres").unwrap().constructs;

    // Without a schema `age > $1` could be text; with it, it is numeric.
    assert!(plain("SELECT id FROM users WHERE age > $1").contains("text_comparison"));
    assert!(!sql("SELECT id FROM users WHERE age > $1").contains("text_comparison"));
    // ... still an ordered comparison.
    assert!(sql("SELECT id FROM users WHERE age > $1").contains("ordered_comparison"));
    // A text column stays conditional, and an unregistered one stays unknown.
    assert!(sql("SELECT id FROM users WHERE name > $1").contains("text_comparison"));
    assert!(sql("SELECT id FROM users WHERE nick > $1").contains("text_comparison"));
    // A qualified reference resolves through the alias; the wrong table's column does not.
    assert!(!sql("SELECT u.id FROM users u WHERE u.age = $1").contains("text_comparison"));
    assert!(sql("SELECT o.id FROM orders o WHERE o.status = $1").contains("text_comparison"));
    // Unqualified column in a join resolves only when one registered table has it.
    assert!(
        !sql("SELECT u.id FROM users u JOIN orders o ON o.user_id = u.id WHERE age = 3")
            .contains("text_comparison")
    );
    // An unregistered table in scope makes an unqualified column unknown (it could be its column).
    assert!(
        sql("SELECT 1 FROM users u JOIN other x ON TRUE WHERE u.id = 1 AND age = $1")
            .contains("text_comparison")
    );
    // A derived table is opaque.
    assert!(
        sql("SELECT d.n FROM (SELECT age AS n FROM users) d WHERE d.n > $1")
            .contains("text_comparison")
    );
    // UPDATE and DELETE scopes.
    assert!(!sql("UPDATE users SET name = 'x' WHERE age > $1").contains("text_comparison"));
    assert!(!sql("DELETE FROM users WHERE age BETWEEN 1 AND 3").contains("text_comparison"));
    // The invariant the gate relies on holds with a schema too: every need a target records is named.
    for target in unisql::dialects() {
        let t = translate(
            "SELECT id FROM users WHERE age > $1 AND name = 'a'",
            "postgres",
            target,
            &all().with_schema(users()),
        );
        if let Ok(t) = t {
            let needs = sql("SELECT id FROM users WHERE age > $1 AND name = 'a'");
            for e in &t.report.entries {
                assert!(needs.contains(&e.construct), "{target}: {}", e.construct);
            }
            for n in &needs {
                assert!(t.report.constructs().contains(n.as_str()), "{target}: {n}");
            }
        }
    }
}

#[test]
fn partial_forms_have_their_own_leaves() {
    let n = |sql: &str, src: &str| analyse(sql, src).unwrap().constructs;
    assert!(
        n(
            "INSERT INTO kv (k, v) VALUES (1, 2) ON CONFLICT (k) DO UPDATE SET v = 2",
            "postgres"
        )
        .contains("upsert")
    );
    assert!(
        !n(
            "INSERT INTO kv (k, v) VALUES (1, 2) ON CONFLICT (k) DO UPDATE SET v = 2",
            "postgres"
        )
        .contains("upsert.no_conflict_key")
    );
    assert!(
        n(
            "INSERT INTO kv (k, v) VALUES (1, 2) ON DUPLICATE KEY UPDATE v = 2",
            "mysql"
        )
        .contains("upsert.no_conflict_key")
    );
    assert!(n("UPDATE t SET a = 1 RETURNING a", "postgres").contains("returning.update"));
    assert!(!n("DELETE FROM t RETURNING a", "postgres").contains("returning.update"));
    assert!(n("SELECT trim(both 'xy' from name) FROM t", "postgres").contains("trim.multi_char"));
    assert!(!n("SELECT trim(both 'x' from name) FROM t", "postgres").contains("trim.multi_char"));
    assert!(
        n(
            "SELECT string_agg(name, ',' ORDER BY age) FROM t",
            "postgres"
        )
        .contains("fn:string_agg.order_by")
    );
    let lock = n(
        "SELECT t.a FROM t JOIN u ON u.a = t.a FOR SHARE OF t SKIP LOCKED",
        "postgres",
    );
    for leaf in ["row_lock.share", "row_lock.of", "row_lock.skip_locked"] {
        assert!(lock.contains(leaf), "{leaf} missing: {lock:?}");
    }
    assert!(!lock.contains("row_lock"), "FOR SHARE is not FOR UPDATE");
    assert!(n("SELECT a FROM t FOR UPDATE NOWAIT", "postgres").contains("row_lock.nowait"));
    assert!(
        n("SELECT DISTINCT ON (a) * FROM t ORDER BY a", "postgres")
            .contains("distinct_on.star_projection")
    );
}

/// Statements (source dialect, text) covering every construct family the translator handles.
fn catalogue() -> &'static [(&'static str, &'static str)] {
    &[
        ("postgres", "SELECT id, name FROM users WHERE id = $1"),
        (
            "postgres",
            "SELECT id FROM users WHERE age > $1 AND city = 'Oslo' ORDER BY age DESC NULLS LAST LIMIT 5 OFFSET 2",
        ),
        (
            "postgres",
            "SELECT name FROM users WHERE name LIKE 'a%' OR email ILIKE '%X'",
        ),
        (
            "postgres",
            "SELECT CAST(id AS TEXT), now(), 7 / 2, 'a' || name FROM users",
        ),
        (
            "postgres",
            "SELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id",
        ),
        (
            "postgres",
            "INSERT INTO kv (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value RETURNING key",
        ),
        (
            "postgres",
            "UPDATE users SET age = age + 1 WHERE city = 'Oslo'",
        ),
        ("postgres", "DELETE FROM orders WHERE amount < 100"),
        (
            "postgres",
            "SELECT DISTINCT ON (city) city, id FROM users ORDER BY city, id",
        ),
        (
            "postgres",
            "SELECT id, row_number() OVER (PARTITION BY city ORDER BY age) FROM users",
        ),
        (
            "postgres",
            "SELECT count(*) FILTER (WHERE status = 'paid') FROM orders",
        ),
        (
            "postgres",
            "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 5) SELECT n FROM t",
        ),
        (
            "postgres",
            "SELECT string_agg(name, ',' ORDER BY age) FROM users",
        ),
        ("postgres", "SELECT id FROM users WHERE id = ANY($1)"),
        ("postgres", "SELECT data->>'a', data->'o' FROM docs"),
        (
            "postgres",
            "SELECT id FROM events WHERE at > now() - make_interval(secs => $1)",
        ),
        (
            "postgres",
            "SELECT trim(both 'x' from name), btrim(name), octet_length(name) FROM users",
        ),
        (
            "postgres",
            "SELECT id FROM users WHERE active = TRUE AND age BETWEEN 20 AND 30",
        ),
        (
            "sqlite",
            "SELECT ifnull(city, 'x') FROM users WHERE name LIKE 'a%' ORDER BY age LIMIT -1 OFFSET 2",
        ),
        (
            "sqlite",
            "INSERT INTO kv (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        ),
        (
            "mysql",
            "SELECT concat(name, '-', ifnull(city, '?')), 7 / 2, 7 DIV 2, length(name) FROM users LIMIT 2, 3",
        ),
        (
            "mysql",
            "SELECT group_concat(name ORDER BY age SEPARATOR '|') FROM users",
        ),
        (
            "mysql",
            "INSERT INTO kv (k, v) VALUES (?, ?) ON DUPLICATE KEY UPDATE v = VALUES(v)",
        ),
        (
            "postgres",
            "SELECT CAST(name AS UUID), CAST(age AS BIGINT) FROM users",
        ),
        (
            "postgres",
            "UPDATE users SET age = 1 WHERE id = $1 RETURNING id",
        ),
        ("postgres", "SELECT trim(both 'xy' from name) FROM users"),
        ("postgres", "SELECT sum(age ORDER BY age) FROM users"),
        (
            "postgres",
            "SELECT DISTINCT ON (city) * FROM users ORDER BY city",
        ),
        ("postgres", "SELECT id FROM users WHERE id = 1 FOR UPDATE"),
        ("postgres", "SELECT id FROM users WHERE id = 1 FOR SHARE"),
        (
            "postgres",
            "SELECT u.id FROM users u JOIN orders o ON o.user_id = u.id FOR UPDATE OF u NOWAIT",
        ),
        (
            "postgres",
            "SELECT id FROM users WHERE age > 3 FOR UPDATE SKIP LOCKED",
        ),
        (
            "mysql",
            "SELECT id FROM users WHERE id = 1 FOR UPDATE NOWAIT",
        ),
        ("postgres", "SELECT md5(name), current_schema() FROM users"),
        (
            "postgres",
            "WITH s AS (SELECT id FROM users WHERE age > 3) UPDATE users SET age = 1 WHERE id IN (SELECT id FROM s)",
        ),
        (
            "postgres",
            "WITH s AS (INSERT INTO kv (key, value) VALUES ('a', 'b') RETURNING key) DELETE FROM users WHERE name IN (SELECT key FROM s)",
        ),
    ]
}

/// The invariant that makes the analyser trustworthy: whatever any target's report says it used, the
/// target-free analysis already named. (The walker may name more; it must never name fewer.)
#[test]
fn analysis_covers_every_construct_a_report_records() {
    let catalogue = catalogue();
    let mut checked = 0;
    for (source, sql) in catalogue {
        let a = analyse(sql, source).unwrap_or_else(|e| panic!("analyse failed for `{sql}`: {e}"));
        assert!(
            a.unreadable.is_none(),
            "`{sql}` unreadable: {:?}",
            a.unreadable
        );
        for target in unisql::dialects() {
            // A refusal carries no report; the leaf it names must be one the analysis already
            // listed, or a gate that looks only at `constructs` would let the statement through.
            if let Err(unisql::Error::Untranslatable(u)) = translate(sql, source, target, &all()) {
                assert!(
                    a.constructs.contains(&u.subject()),
                    "`{sql}` ({source} -> {target}) is refused as `{}` but analyse = {:?}",
                    u.subject(),
                    a.constructs
                );
                assert!(a.refused.contains(&u.subject()), "{:?}", a.refused);
            }
            if let Ok(t) = translate(sql, source, target, &all()) {
                for e in &t.report.entries {
                    assert!(
                        a.constructs.contains(&e.construct),
                        "`{sql}` ({source} -> {target}): report names `{}` but analyse() does not; analyse = {:?}",
                        e.construct,
                        a.constructs
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(
        checked > 100,
        "the invariant looked at only {checked} report entries"
    );
}

/// The directions that ship a matrix file: a gate for these reads the file.
const MATRIX_PAIRS: &[(&str, &str)] = &[
    ("postgres", "sqlite"),
    ("sqlite", "postgres"),
    ("postgres", "mysql"),
    ("postgres", "mariadb"),
    ("mysql", "postgres"),
    ("mysql", "sqlite"),
    ("mariadb", "postgres"),
];

/// A gate that requires every leaf `analyse` names for a statement must find each one recorded when
/// the target translates it: a statement that translates is accepted leaf by leaf, so a need with no
/// recorded leaf would make the gate refuse what the translator gives (too strict). The other
/// direction is `analysis_covers_every_construct_a_report_records`.
#[test]
fn every_need_of_a_translated_statement_is_recorded_for_that_target() {
    let mut checked = 0;
    let mut gaps = Vec::new();
    for (source, sql) in catalogue() {
        let needs = analyse(sql, source).unwrap().constructs;
        for (s, target) in MATRIX_PAIRS {
            // A MariaDB/MySQL text is read by the one MySQL reader; pair on the dialect it names.
            let same_reader = s == source || (*s == "mariadb" && *source == "mysql");
            if !same_reader {
                continue;
            }
            if let Ok(t) = translate(sql, s, target, &all()) {
                let have: std::collections::BTreeSet<&str> = t.report.constructs();
                let missing: Vec<&String> = needs
                    .iter()
                    .filter(|n| !have.contains(n.as_str()))
                    .collect();
                checked += 1;
                if !missing.is_empty() {
                    gaps.push(format!("{s}->{target} `{sql}`: {missing:?}"));
                }
            }
        }
    }
    assert!(checked > 20, "looked at only {checked} translations");
    assert!(
        gaps.is_empty(),
        "needs with no recorded leaf:
{}",
        gaps.join(
            "
"
        )
    );
}
