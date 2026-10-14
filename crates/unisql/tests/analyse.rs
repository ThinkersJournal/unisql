// SPDX-License-Identifier: MIT OR Apache-2.0
//! `analyse` is target-free: it reports what a statement needs, and which of its forms some target
//! refuses. No engine is involved.

use unisql::{Allowance, Options, analyse, translate};

fn all() -> Options {
    Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
}

#[test]
fn a_plain_cast_needs_cast_and_nothing_refused() {
    let a = analyse("SELECT CAST(id AS TEXT) FROM users", "postgres").unwrap();
    assert!(a.constructs.contains("cast"));
    assert!(!a.refused.contains("cast.refused"), "{:?}", a.refused);
    assert_eq!(a.unreadable, None);
}

#[test]
fn a_cast_to_uuid_yields_the_refused_subject() {
    let a = analyse("SELECT CAST(name AS UUID) FROM users", "postgres").unwrap();
    assert!(a.refused.contains("cast.refused"), "{:?}", a.refused);
}

#[test]
fn forms_refused_by_some_target_are_listed() {
    let lock = analyse("SELECT id FROM users WHERE id = 1 FOR UPDATE", "postgres").unwrap();
    assert!(lock.constructs.contains("row_lock"));
    assert!(
        lock.refused.contains("row_lock.refused"),
        "{:?}",
        lock.refused
    );

    let md5 = analyse("SELECT md5(name) FROM users", "postgres").unwrap();
    assert!(md5.refused.contains("fn:md5.refused"), "{:?}", md5.refused);

    let star = analyse(
        "SELECT DISTINCT ON (city) * FROM users ORDER BY city",
        "postgres",
    )
    .unwrap();
    assert!(
        star.refused.contains("distinct_on.refused"),
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

/// The invariant that makes the analyser trustworthy: whatever any target's report says it used, the
/// target-free analysis already named. (The walker may name more; it must never name fewer.)
#[test]
fn analysis_covers_every_construct_a_report_records() {
    let catalogue: &[(&str, &str)] = &[
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
    ];
    let mut checked = 0;
    for (source, sql) in catalogue {
        let a = analyse(sql, source).unwrap_or_else(|e| panic!("analyse failed for `{sql}`: {e}"));
        assert!(
            a.unreadable.is_none(),
            "`{sql}` unreadable: {:?}",
            a.unreadable
        );
        for target in unisql::dialects() {
            // Refusals are fine: they carry no report, and are accounted for in `refused`.
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
