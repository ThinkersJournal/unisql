// SPDX-License-Identifier: MIT OR Apache-2.0
//! The schema snapshot file and the column facts a DDL script yields.

use unisql::ast::Name;
use unisql::{ColumnClass, Schema, schema_from_ddl};
use unisql_macro_core::{SNAPSHOT_FORMAT, Snapshot, SnapshotError};

const PG: &str = "
CREATE TABLE users (
    id serial PRIMARY KEY,
    email varchar(64) NOT NULL,
    nick text,
    score bigint DEFAULT 0,
    note text NOT NULL DEFAULT ''
);
CREATE TABLE pairs (a int, b int, c text, PRIMARY KEY (a, b), UNIQUE (c));
ALTER TABLE users ALTER COLUMN nick SET NOT NULL;
ALTER TABLE users ALTER COLUMN note DROP DEFAULT;
ALTER TABLE users ALTER COLUMN score TYPE numeric;
";

fn schema() -> Schema {
    schema_from_ddl(PG, "postgres").unwrap().schema
}

fn facts(s: &Schema, table: &str, column: &str) -> (Option<bool>, Option<bool>, Option<String>) {
    let c = s.get(&Name::simple(table), column).expect("column");
    (c.nullable, c.has_default, c.declared_type.clone())
}

#[test]
fn ddl_yields_declared_type_nullability_and_default() {
    let s = schema();
    // serial: NOT NULL with a default; a column-level primary key is NOT NULL
    assert_eq!(facts(&s, "users", "id").0, Some(false));
    assert_eq!(facts(&s, "users", "id").1, Some(true));
    assert_eq!(
        facts(&s, "users", "email"),
        (Some(false), Some(false), Some("VARCHAR(64)".into()))
    );
    // ALTER .. SET NOT NULL applied; DROP DEFAULT applied
    assert_eq!(facts(&s, "users", "nick").0, Some(false));
    assert_eq!(facts(&s, "users", "note").1, Some(false));
    // SET DATA TYPE keeps nullability and default and changes the type
    let (nullable, default, ty) = facts(&s, "users", "score");
    assert_eq!((nullable, default), (Some(true), Some(true)));
    assert_eq!(ty.as_deref(), Some("NUMERIC"));
    // a table-level PRIMARY KEY makes its columns NOT NULL; a UNIQUE one does not
    assert_eq!(facts(&s, "pairs", "a").0, Some(false));
    assert_eq!(facts(&s, "pairs", "b").0, Some(false));
    assert_eq!(facts(&s, "pairs", "c").0, Some(true));
}

#[test]
fn mysql_auto_increment_is_a_default() {
    let s = schema_from_ddl(
        "CREATE TABLE t (id BIGINT AUTO_INCREMENT PRIMARY KEY, v INT);",
        "mysql",
    )
    .unwrap()
    .schema;
    assert_eq!(facts(&s, "t", "id").1, Some(true));
    assert_eq!(facts(&s, "t", "v").1, Some(false));
}

#[test]
fn tables_enumerate_with_keys_and_completeness() {
    let s = schema();
    let names: Vec<_> = s.tables().map(|t| t.name.join(".")).collect();
    assert_eq!(names, ["pairs", "users"]);
    let pairs = s.tables().find(|t| t.name == ["pairs"]).unwrap();
    assert_eq!(pairs.columns.len(), 3);
    assert!(pairs.columns_complete && !pairs.keys_unreliable);
    assert_eq!(pairs.unique_keys, [vec!["a", "b"], vec!["c"]]);
    assert_eq!(
        pairs.columns["c"].class,
        ColumnClass::Text,
        "columns are reachable by name"
    );
}

#[test]
fn a_snapshot_round_trips_and_is_stable() {
    let mut snap = Snapshot::new(schema());
    snap.meta
        .insert("migration.0001".into(), "sha256:abc".into());
    let text = snap.to_json();
    assert_eq!(text, snap.to_json());
    assert!(
        text.contains("\"format\": 1") && text.ends_with("}\n"),
        "{text}"
    );
    let back = Snapshot::from_json(&text).unwrap();
    assert_eq!(back, snap);
    assert_eq!(back.schema.tables().count(), 2);
    assert_eq!(
        back.schema
            .unique_keys(&Name::simple("pairs"))
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn a_newer_format_is_refused_before_its_shape_is_read() {
    // a future file: `format` 2 and a `schema` this version could not parse
    let err = Snapshot::from_json(r#"{"format": 2, "schema": {"whatever": 1}}"#).unwrap_err();
    assert_eq!(
        err,
        SnapshotError::TooNew {
            found: SNAPSHOT_FORMAT + 1,
            supported: SNAPSHOT_FORMAT
        }
    );
    assert!(matches!(
        Snapshot::from_json("not json"),
        Err(SnapshotError::Malformed(_))
    ));
    assert!(matches!(
        Snapshot::from_json(r#"{"format": 1}"#),
        Err(SnapshotError::Malformed(_))
    ));
}

#[test]
fn missing_optional_fields_read_as_unknown_and_extra_fields_are_ignored() {
    let text = r#"{
        "format": 1,
        "future_top_level": true,
        "schema": {"tables": [
            {"name": ["t"], "extra": 1, "columns": {"c": {"class": "Numeric", "collation": null}}}
        ]}
    }"#;
    let snap = Snapshot::from_json(text).unwrap();
    let c = snap.schema.get(&Name::simple("t"), "c").unwrap();
    assert_eq!(c.class, ColumnClass::Numeric);
    assert_eq!(
        (c.declared_type.clone(), c.nullable, c.has_default),
        (None, None, None)
    );
    // a missing optional list or flag is "not stated", never "complete"
    assert!(snap.schema.complete_columns(&Name::simple("t")).is_none());
}
