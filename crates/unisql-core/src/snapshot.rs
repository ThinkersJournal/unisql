// SPDX-License-Identifier: MIT OR Apache-2.0
//! A [`Schema`] as a file: the schema snapshot a project commits beside its code (`docs/MACROS.md`
//! section 7). Behind the `serde` feature.
//!
//! The file is JSON with a `format` integer. A reader refuses a `format` newer than it understands, so
//! an old toolchain fails loudly on a new file instead of silently dropping what it cannot read. Within
//! one format, unknown fields in a table or column are ignored and missing optional ones read as
//! unknown (`None`), so adding a field is not a format change.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::schema::{Column, Schema};

/// The format number this version writes and the newest it reads.
pub const SNAPSHOT_FORMAT: u32 = 1;

/// A schema plus free-form text the producer wants kept with it (PersistAnt records the migration
/// versions and checksums the snapshot was derived from).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
    pub schema: Schema,
}

/// Why a snapshot could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// The text is not a snapshot (not JSON, or a field has the wrong shape).
    Malformed(String),
    /// The file was written by a newer UniSQL than this one.
    TooNew { found: u32, supported: u32 },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Malformed(e) => write!(f, "not a schema snapshot: {e}"),
            SnapshotError::TooNew { found, supported } => write!(
                f,
                "schema snapshot format {found} is newer than this UniSQL reads (format {supported}); update unisql"
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl Snapshot {
    pub fn new(schema: Schema) -> Self {
        Snapshot {
            format: SNAPSHOT_FORMAT,
            meta: BTreeMap::new(),
            schema,
        }
    }

    /// Pretty-printed JSON with a trailing newline, stable for a given snapshot (tables, columns and
    /// meta are in name order), so a committed file only changes when the schema does.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a snapshot always serialises");
        text.push('\n');
        text
    }

    pub fn from_json(text: &str) -> Result<Snapshot, SnapshotError> {
        // Read the format first: a newer file may have a shape this version cannot parse at all.
        #[derive(Deserialize)]
        struct Head {
            format: u32,
        }
        let head: Head =
            serde_json::from_str(text).map_err(|e| SnapshotError::Malformed(e.to_string()))?;
        if head.format > SNAPSHOT_FORMAT {
            return Err(SnapshotError::TooNew {
                found: head.format,
                supported: SNAPSHOT_FORMAT,
            });
        }
        serde_json::from_str(text).map_err(|e| SnapshotError::Malformed(e.to_string()))
    }
}

/// The wire shape of a [`Schema`]: a list of tables, because a table name is a list of strings and a
/// JSON object key cannot be.
#[derive(Serialize, Deserialize)]
pub(crate) struct SchemaRepr {
    tables: Vec<TableRepr>,
}

#[derive(Serialize, Deserialize)]
struct TableRepr {
    name: Vec<String>,
    columns: BTreeMap<String, Column>,
    #[serde(default)]
    unique_keys: Vec<Vec<String>>,
    #[serde(default)]
    keys_unreliable: bool,
    #[serde(default)]
    columns_complete: bool,
}

impl From<Schema> for SchemaRepr {
    fn from(s: Schema) -> Self {
        SchemaRepr {
            tables: s
                .tables()
                .map(|t| TableRepr {
                    name: t.name.to_vec(),
                    columns: t.columns.clone(),
                    unique_keys: t.unique_keys.to_vec(),
                    keys_unreliable: t.keys_unreliable,
                    columns_complete: t.columns_complete,
                })
                .collect(),
        }
    }
}

impl From<SchemaRepr> for Schema {
    fn from(r: SchemaRepr) -> Self {
        let mut s = Schema::new();
        for t in r.tables {
            for (name, column) in t.columns {
                s.set_column(t.name.clone(), name, column);
            }
            for key in t.unique_keys {
                s.add_unique_key(t.name.clone(), key);
            }
            if t.keys_unreliable {
                s.mark_keys_unreliable(t.name.clone());
            }
            if t.columns_complete {
                s.mark_columns_complete(t.name);
            }
        }
        s
    }
}
