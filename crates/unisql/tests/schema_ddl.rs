// SPDX-License-Identifier: MIT OR Apache-2.0
//! `schema_from_ddl` fills a `Schema` from DDL text; the translator then treats comparisons on the
//! columns it classified exactly as it does for a hand-built `Schema`.

use unisql::ast::Name;
use unisql::{ColumnClass, Options, analyse_with, schema_from_ddl};

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
