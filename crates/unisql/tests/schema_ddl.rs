// SPDX-License-Identifier: MIT OR Apache-2.0
//! `schema_from_ddl` fills a `Schema` from DDL text; the translator then treats comparisons on the
//! columns it classified exactly as it does for a hand-built `Schema`.

use unisql::ast::Name;
use unisql::{Allowance, ColumnClass, Options, analyse_with, schema_from_ddl};

const PG: &str = r#"
CREATE TABLE users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL COLLATE "C",
    age integer,
    active boolean NOT NULL DEFAULT false,
    nick citext,
    tags text[],
    born date
);
CREATE TABLE orders (id serial PRIMARY KEY, user_id integer NOT NULL, status varchar(16) NOT NULL);
ALTER TABLE users ADD COLUMN score double precision;
ALTER TABLE users DROP COLUMN born;
ALTER TABLE orders ALTER COLUMN status TYPE text;
CREATE FUNCTION touch() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TABLE scratch (x integer);
DROP TABLE scratch;
THIS IS NOT SQL;
"#;

fn needs(opts: &Options, sql: &str) -> std::collections::BTreeSet<String> {
    analyse_with(sql, "postgres", opts).unwrap().constructs
}

#[test]
fn postgres_ddl_classifies_columns_and_survives_function_bodies() {
    let d = schema_from_ddl(PG, "postgres").unwrap();
    // the garbage statement is reported, not fatal; the dollar-quoted body did not split
    assert!(
        d.skipped.iter().any(|s| s.starts_with("THIS IS NOT")),
        "{:?}",
        d.skipped
    );
    let users = Name::simple("users");
    let class = |t: &Name, c: &str| d.schema.get(t, c).map(|c| c.class);
    assert_eq!(class(&users, "age"), Some(ColumnClass::Numeric));
    assert_eq!(class(&users, "name"), Some(ColumnClass::Text));
    assert_eq!(class(&users, "active"), Some(ColumnClass::Boolean));
    assert_eq!(class(&users, "id"), Some(ColumnClass::Other));
    assert_eq!(class(&users, "score"), Some(ColumnClass::Numeric));
    // a custom type and an array are registered as unknown, never omitted or guessed
    assert_eq!(class(&users, "nick"), Some(ColumnClass::Unknown));
    assert_eq!(class(&users, "tags"), Some(ColumnClass::Unknown));
    // ALTER ... DROP COLUMN, ALTER ... TYPE and DROP TABLE were applied in order
    assert_eq!(class(&users, "born"), None);
    assert_eq!(
        class(&Name::simple("orders"), "status"),
        Some(ColumnClass::Text)
    );
    assert!(d.schema.get(&Name::simple("scratch"), "x").is_none());
    // the collation is only what the column states
    assert_eq!(
        d.schema
            .get(&users, "name")
            .and_then(|c| c.collation.clone())
            .as_deref(),
        Some("C")
    );
    assert_eq!(d.schema.get(&users, "age").unwrap().collation, None);
}

#[test]
fn a_ddl_schema_drives_the_same_decisions_as_a_hand_built_one() {
    let opts = Options::default().with_schema(schema_from_ddl(PG, "postgres").unwrap().schema);
    let n = |q: &str| needs(&opts, q);
    assert!(!n("SELECT id FROM users WHERE age > $1").contains("text_comparison"));
    assert!(!n("SELECT id FROM users WHERE score > $1").contains("text_comparison"));
    assert!(n("SELECT id FROM users WHERE name > $1").contains("text_comparison"));
    // unknown-typed and dropped columns stay conservative
    assert!(n("SELECT id FROM users WHERE nick = $1").contains("text_comparison"));
    assert!(n("SELECT id FROM users WHERE born = $1").contains("text_comparison"));
    // an unqualified column that is unknown in one table is not taken for the other's
    assert!(
        n("SELECT 1 FROM users u JOIN orders o ON o.user_id = u.age WHERE nick = $1")
            .contains("text_comparison")
    );
}

#[test]
fn mysql_ddl_keeps_tinyint_numeric_and_reads_column_collation() {
    let d = schema_from_ddl(
        "CREATE TABLE t (a TINYINT(1), b BOOLEAN, c VARCHAR(10) COLLATE utf8mb4_bin, d DATETIME) ENGINE=InnoDB;",
        "mysql",
    )
    .unwrap();
    let t = Name::simple("t");
    let get = |c: &str| d.schema.get(&t, c).unwrap().clone();
    assert_eq!(get("a").class, ColumnClass::Numeric);
    assert_eq!(get("b").class, ColumnClass::Boolean);
    assert_eq!(get("c").class, ColumnClass::Text);
    assert_eq!(get("c").collation.as_deref(), Some("utf8mb4_bin"));
    assert_eq!(get("d").class, ColumnClass::Other);
}

#[test]
fn an_unknown_dialect_is_an_error() {
    assert!(schema_from_ddl("CREATE TABLE t (a int)", "klingon").is_err());
}

#[test]
fn a_non_bytewise_column_collation_needs_its_own_allowance_across_dialects() {
    use unisql::{Allowance, Column, Schema, translate};
    let schema = Schema::new()
        .with_column("t", "c", Column::text().with_collation("en_US"))
        .with_column("t", "b", Column::text().with_collation("C"))
        .with_column("t", "u", Column::text());
    let base = Options::default()
        .with(Allowance::TextCollation)
        .with(Allowance::LikeCaseSensitivityIsConnectionSetting)
        .with_schema(schema);
    let run = |q: &str, o: &Options, target: &str| translate(q, "postgres", target, o);

    // a locale collation: refused unless that divergence is accepted by name
    let err = run("SELECT 1 FROM t WHERE c = $1", &base, "sqlite").unwrap_err();
    assert!(
        err.to_string().contains("source_collation_not_bytewise"),
        "{err}"
    );
    let ok = base.clone().with(Allowance::SourceCollationNotBytewise);
    let t = run("SELECT 1 FROM t WHERE c = $1", &ok, "sqlite").unwrap();
    assert!(
        t.report
            .entries
            .iter()
            .any(|e| e.allowance == Some(Allowance::SourceCollationNotBytewise)),
        "{:?}",
        t.report
    );
    // ORDER BY and BETWEEN too
    assert!(run("SELECT c FROM t ORDER BY c", &base, "mysql").is_err());
    assert!(
        run(
            "SELECT c FROM t WHERE c BETWEEN 'a' AND 'b'",
            &base,
            "mysql"
        )
        .is_err()
    );
    assert!(run("SELECT c FROM t ORDER BY c", &ok, "mysql").is_ok());
    // a bytewise collation and an unstated one (unknown, not "the default") need nothing extra
    assert!(run("SELECT 1 FROM t WHERE b = $1", &base, "sqlite").is_ok());
    assert!(run("SELECT 1 FROM t WHERE u = $1", &base, "sqlite").is_ok());
    // the same dialect in and out is the identity: no divergence to accept
    assert!(run("SELECT 1 FROM t WHERE c = $1", &base, "postgres").is_ok());
    // no schema, no knowledge: behaves as before
    let plain = Options::default().with(Allowance::TextCollation);
    assert!(run("SELECT 1 FROM t WHERE c = $1", &plain, "sqlite").is_ok());
}

#[test]
fn bytewise_collation_names() {
    use unisql::is_bytewise_collation as b;
    for n in [
        "C",
        "c",
        "POSIX",
        "ucs_basic",
        "BINARY",
        "utf8mb4_bin",
        "\"C\"",
        "latin1_bin",
    ] {
        assert!(b(n), "{n}");
    }
    for n in [
        "en_US",
        "en_US.utf8",
        "NOCASE",
        "utf8mb4_0900_ai_ci",
        "utf8mb4_general_ci",
        "und-x-icu",
    ] {
        assert!(!b(n), "{n}");
    }
}

/// A quoted identifier in DDL reads (it used to panic: the reader context was not installed).
#[test]
fn quoted_identifiers_in_ddl_are_read() {
    let m = schema_from_ddl(
        "CREATE TABLE `kv` (`key` VARCHAR(10) NOT NULL PRIMARY KEY, `Value` TEXT);",
        "mysql",
    )
    .unwrap();
    let t = Name::simple("kv");
    assert_eq!(m.schema.get(&t, "key").unwrap().class, ColumnClass::Text);
    // MySQL column names are case-insensitive: a quoted name folds.
    assert_eq!(m.schema.get(&t, "value").unwrap().class, ColumnClass::Text);
    let p = schema_from_ddl(r#"CREATE TABLE kv ("Key" text, n integer);"#, "postgres").unwrap();
    // PostgreSQL keeps a quoted name's case.
    assert_eq!(p.schema.get(&t, "Key").unwrap().class, ColumnClass::Text);
}

const AUTH_MYSQL: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    session_id VARCHAR(255) NOT NULL PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    data JSON NOT NULL,
    expires_at DATETIME(6),
    INDEX idx_sessions_user_id (user_id)
);
CREATE TABLE kv_store (`key` VARCHAR(512) NOT NULL PRIMARY KEY, value LONGBLOB NOT NULL);
CREATE TABLE two (a INT PRIMARY KEY, b INT, c INT, UNIQUE KEY ub (b));
CREATE TABLE composite (a INT, b INT, v INT, PRIMARY KEY (a, b));
CREATE TABLE partial (a INT PRIMARY KEY, b INT);
CREATE UNIQUE INDEX p_b ON partial (b) WHERE b > 0;
CREATE TABLE ix (a INT, b INT);
CREATE UNIQUE INDEX ix_a ON ix (a);
CREATE TABLE nokey (a INT, b INT);
";

#[test]
fn ddl_reads_unique_keys_and_an_upsert_without_a_target_takes_the_sole_key() {
    use unisql::{Allowance, translate};
    let d = schema_from_ddl(AUTH_MYSQL, "mysql").unwrap();
    let keys = |t: &str| d.schema.unique_keys(&Name::simple(t)).map(|k| k.to_vec());
    assert_eq!(keys("sessions"), Some(vec![vec!["session_id".to_string()]]));
    assert_eq!(keys("kv_store"), Some(vec![vec!["key".to_string()]]));
    assert_eq!(keys("two").map(|k| k.len()), Some(2));
    assert_eq!(
        keys("composite"),
        Some(vec![vec!["a".to_string(), "b".to_string()]])
    );
    assert_eq!(keys("partial"), None, "a partial unique index: unreliable");
    assert_eq!(keys("ix"), Some(vec![vec!["a".to_string()]]));
    assert_eq!(keys("nokey"), None);

    let opts = Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
        .with_schema(d.schema);
    let go = |sql: &str, target: &str| translate(sql, "mysql", target, &opts);
    let up = "INSERT INTO kv_store (`key`, `value`) VALUES (?, ?) ON DUPLICATE KEY UPDATE `value` = VALUES(`value`)";
    assert_eq!(
        go(up, "postgres").unwrap().sql,
        r#"INSERT INTO "kv_store" ("key", "value") VALUES ($1, $2) ON CONFLICT ("key") DO UPDATE SET "value" = EXCLUDED."value""#
    );
    assert!(
        go(up, "sqlite")
            .unwrap()
            .sql
            .contains(r#"ON CONFLICT ("key") DO UPDATE"#)
    );
    // a composite key is written in key order
    let c = go(
        "INSERT INTO composite (a, b, v) VALUES (1, 2, 3) ON DUPLICATE KEY UPDATE v = VALUES(v)",
        "postgres",
    )
    .unwrap();
    assert!(c.sql.contains(r#"ON CONFLICT ("a", "b")"#), "{}", c.sql);
    // several unique keys, an unreliable list, no stated key: refused with the same leaf
    for t in ["two", "partial", "nokey"] {
        let r = go(
            &format!("INSERT INTO {t} (a, b) VALUES (1, 2) ON DUPLICATE KEY UPDATE b = VALUES(b)"),
            "postgres",
        );
        assert!(
            matches!(&r, Err(unisql::Error::Untranslatable(u)) if u.construct == "upsert.no_conflict_key"),
            "{t}: {r:?}"
        );
    }
    // MySQL as target is untouched (it fires on any key and needs none)
    let m = go(up, "mariadb").unwrap();
    assert!(m.sql.contains("ON DUPLICATE KEY UPDATE"), "{}", m.sql);
    // DROP INDEX makes every table's list unreliable (it names no table)
    let d2 = schema_from_ddl(
        "CREATE TABLE t (a INT PRIMARY KEY); DROP INDEX some_idx ON other;",
        "mysql",
    )
    .unwrap();
    assert_eq!(d2.schema.unique_keys(&Name::simple("t")), None);
}

#[test]
fn an_upsert_where_on_numeric_columns_names_no_text_comparison() {
    use unisql::translate;
    let s = schema_from_ddl(
        "CREATE TABLE t (id BIGINT PRIMARY KEY, n BIGINT NOT NULL, s TEXT);",
        "postgres",
    )
    .unwrap();
    let o = Options::default().with_schema(s.schema);
    let pg = |w: &str| {
        format!(
            "INSERT INTO t (id, n) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET n = EXCLUDED.n WHERE {w}"
        )
    };
    let n = analyse_with(&pg("t.n < EXCLUDED.n"), "postgres", &o)
        .unwrap()
        .constructs;
    assert!(!n.contains("text_comparison"), "{n:?}");
    // control: a text column in the same position still names it
    let n = analyse_with(&pg("t.s < EXCLUDED.s"), "postgres", &o)
        .unwrap()
        .constructs;
    assert!(n.contains("text_comparison"), "{n:?}");
    // translated with the two upsert allowances only: no `text_collation` is demanded
    let o = o
        .with(Allowance::UpsertWhereAsIf)
        .with(Allowance::UpsertAnyUniqueKey);
    let t = translate(&pg("t.n < EXCLUDED.n"), "postgres", "mysql", &o).unwrap();
    assert!(t.sql.contains("IF("), "{}", t.sql);
}

#[test]
fn upsert_set_columns_are_qualified_with_the_table_but_a_subquerys_are_not() {
    use unisql::translate;
    let s = schema_from_ddl(
        "CREATE TABLE kv (k VARCHAR(9) PRIMARY KEY, n INT);",
        "mysql",
    )
    .unwrap();
    let o = Options::default().with_schema(s.schema);
    let t = translate(
        "INSERT INTO kv (k, n) VALUES (?, 1) ON DUPLICATE KEY UPDATE n = n + (SELECT MAX(n) FROM kv)",
        "mysql",
        "postgres",
        &o,
    )
    .unwrap();
    assert!(
        t.sql
            .contains(r#"SET "n" = "kv"."n" + (SELECT MAX("n") FROM "kv")"#),
        "{}",
        t.sql
    );
}

#[test]
fn ordering_a_bytes_column_needs_no_text_collation() {
    use unisql::translate;
    let d = schema_from_ddl(
        "CREATE TABLE kv (k BYTEA PRIMARY KEY, v BYTEA NOT NULL, name TEXT)",
        "postgres",
    )
    .unwrap();
    let opts = Options::default().with_schema(d.schema);
    let range = "SELECT k FROM kv WHERE k >= $1 AND k < $2 ORDER BY k LIMIT $3";
    for target in ["sqlite", "mysql"] {
        translate(range, "postgres", target, &opts).unwrap_or_else(|e| panic!("{target}: {e}"));
        // a text column still orders by the target's collation
        let err =
            translate("SELECT k FROM kv ORDER BY name", "postgres", target, &opts).unwrap_err();
        assert!(
            err.to_string().contains("text_collation"),
            "{target}: {err}"
        );
    }
}
