// SPDX-License-Identifier: MIT OR Apache-2.0
//! What a caller knows about its tables: the class (and collation) of base-table columns.
//!
//! A [`Schema`] is pure data: UniSQL does no I/O to get one. It is optional, and without it the
//! translator decides numeric-versus-text from syntax alone (a bare column or parameter is "unknown",
//! so a comparison involving it is treated as a possible text comparison). With it, a comparison
//! whose operand is a column of a class other than text involves no text collation, and the
//! assumption the caller would otherwise have to grant (`text_collation`) is not needed for it.
//!
//! Scope and alias resolution stay inside UniSQL, which parses the statement; the schema only supplies
//! facts about **base tables**. Columns of views, CTEs, derived tables, aliases and expressions are
//! never looked up, so they stay unknown.
//!
//! Names are matched exactly as the reader produces them (unquoted identifiers lower-cased, quoted
//! ones as written). A table registered as `users` matches `users` in a statement and not
//! `public.users`: UniSQL does not know the search path, so an unqualified/qualified mismatch is
//! "unknown", never a guess.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Expr, Name, TableRef};

/// The class of a column, as far as comparing it is concerned.
///
/// `Boolean` should be given only when the declared type says boolean (not a `tinyint(1)`); `Other`
/// covers dates, UUIDs, JSON, bytes and anything else without a text collation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ColumnClass {
    Text,
    Numeric,
    Boolean,
    Other,
    /// The column exists but its type is not one UniSQL classifies (a custom type, an array): it is
    /// recorded so that an unqualified reference cannot be taken for another table's column.
    Unknown,
}

/// One column: its class and, when the caller knows it, its collation (carried as given; UniSQL does
/// not interpret it yet).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Column {
    pub class: ColumnClass,
    pub collation: Option<String>,
}

impl Column {
    pub fn new(class: ColumnClass) -> Self {
        Column {
            class,
            collation: None,
        }
    }

    pub fn text() -> Self {
        Column::new(ColumnClass::Text)
    }

    pub fn numeric() -> Self {
        Column::new(ColumnClass::Numeric)
    }

    pub fn boolean() -> Self {
        Column::new(ColumnClass::Boolean)
    }

    pub fn other() -> Self {
        Column::new(ColumnClass::Other)
    }

    pub fn unknown() -> Self {
        Column::new(ColumnClass::Unknown)
    }

    pub fn with_collation(mut self, collation: impl Into<String>) -> Self {
        self.collation = Some(collation.into());
        self
    }
}

/// Base tables and their columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schema {
    tables: BTreeMap<Vec<String>, BTreeMap<String, Column>>,
    /// Unique keys (primary or unique constraint, unique index) per table, columns in key order.
    keys: BTreeMap<Vec<String>, Vec<Vec<String>>>,
    /// Tables whose key list must not be trusted as complete (an index the reader could not
    /// interpret, such as a partial or expression index).
    keys_unreliable: BTreeSet<Vec<String>>,
    /// Tables whose column list is stated in full (read from a `CREATE TABLE`, or marked).
    columns_complete: BTreeSet<Vec<String>>,
}

impl Schema {
    pub fn new() -> Self {
        Schema::default()
    }

    /// Register `column` of the unqualified table `table`.
    pub fn with_column(self, table: &str, column: &str, c: Column) -> Self {
        self.with_qualified_column(&[table], column, c)
    }

    /// Register `column` of the table named by `table` (`["public", "users"]` for `public.users`).
    pub fn with_qualified_column(mut self, table: &[&str], column: &str, c: Column) -> Self {
        self.tables
            .entry(table.iter().map(|s| s.to_string()).collect())
            .or_default()
            .insert(column.to_string(), c);
        self
    }

    /// Register or replace a column (the mutating form of [`Schema::with_qualified_column`]).
    pub fn set_column(&mut self, table: Vec<String>, column: String, c: Column) {
        self.tables.entry(table).or_default().insert(column, c);
    }

    /// Forget one column (an `ALTER TABLE .. DROP COLUMN`); a key that used it goes with it.
    pub fn drop_column(&mut self, table: &Name, column: &str) {
        if let Some(t) = self.tables.get_mut(&table.0) {
            t.remove(column);
        }
        if let Some(ks) = self.keys.get_mut(&table.0) {
            ks.retain(|k| !k.iter().any(|c| c == column));
        }
    }

    /// Forget a table (a `DROP TABLE`).
    pub fn drop_table(&mut self, table: &Name) {
        self.tables.remove(&table.0);
        self.keys.remove(&table.0);
        self.keys_unreliable.remove(&table.0);
        self.columns_complete.remove(&table.0);
    }

    /// State one unique key (primary key, unique constraint or unique index, **not** a partial or
    /// expression index) of the unqualified table `table`, columns in key order. **State every unique
    /// key the table has**: the translator treats the stated keys as the complete list. A table with
    /// no stated key has unknown keys, not "no keys".
    pub fn with_unique_key(mut self, table: &str, columns: &[&str]) -> Self {
        self.add_unique_key(
            vec![table.to_string()],
            columns.iter().map(|c| c.to_string()).collect(),
        );
        self
    }

    /// The mutating form of [`Schema::with_unique_key`] (qualified table name, owned columns).
    pub fn add_unique_key(&mut self, table: Vec<String>, columns: Vec<String>) {
        let ks = self.keys.entry(table).or_default();
        if !ks.contains(&columns) {
            ks.push(columns);
        }
    }

    /// Say that this table has a unique index the reader could not express as a column list, so
    /// its key list is incomplete and no conflict key is derived from it.
    pub fn mark_keys_unreliable(&mut self, table: Vec<String>) {
        self.keys_unreliable.insert(table);
    }

    /// As [`Schema::mark_keys_unreliable`] for every table (a `DROP INDEX` names no table).
    pub fn mark_all_keys_unreliable(&mut self) {
        let all: Vec<Vec<String>> = self.keys.keys().cloned().collect();
        self.keys_unreliable.extend(all);
    }

    /// Say that the columns registered for `table` are all it has (a `REPLACE INTO` written as an
    /// upsert sets every column, so it needs the full list). `schema_from_ddl` does this for each
    /// `CREATE TABLE` it reads.
    pub fn mark_columns_complete(&mut self, table: Vec<String>) {
        self.columns_complete.insert(table);
    }

    /// All column names of `table`, when its column list is marked complete.
    pub fn complete_columns(&self, table: &Name) -> Option<Vec<&str>> {
        if !self.columns_complete.contains(&table.0) {
            return None;
        }
        Some(
            self.tables
                .get(&table.0)?
                .keys()
                .map(String::as_str)
                .collect(),
        )
    }

    /// The stated unique keys of `table`; `None` when none were stated or the list is unreliable.
    pub fn unique_keys(&self, table: &Name) -> Option<&[Vec<String>]> {
        if self.keys_unreliable.contains(&table.0) {
            return None;
        }
        self.keys
            .get(&table.0)
            .map(Vec::as_slice)
            .filter(|k| !k.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// The column, when the table is registered and has it.
    pub fn get(&self, table: &Name, column: &str) -> Option<&Column> {
        self.tables.get(&table.0)?.get(column)
    }

    fn knows_table(&self, table: &Name) -> bool {
        self.tables.contains_key(&table.0)
    }
}

/// The tables a statement's expressions can refer to, innermost query last. Used by the printer and
/// by the needs walker, so both decide "is this operand a text column" the same way.
#[derive(Clone, Debug, Default)]
pub struct Scopes<'a> {
    schema: Option<&'a Schema>,
    /// Each scope: (binding name, the base table when it is one).
    stack: Vec<Vec<(String, Option<Name>)>>,
}

impl<'a> Scopes<'a> {
    pub fn new(schema: Option<&'a Schema>) -> Self {
        Scopes {
            schema,
            stack: Vec::new(),
        }
    }

    /// Enter a query whose FROM list is `from`.
    pub fn push_from(&mut self, from: &[TableRef]) {
        if self.schema.is_none() {
            return;
        }
        let mut scope = Vec::new();
        for t in from {
            collect(t, &mut scope);
        }
        self.stack.push(scope);
    }

    /// Enter an UPDATE/DELETE: the target table (with its alias) and any FROM/USING tables.
    pub fn push_target(&mut self, table: &Name, alias: Option<&str>, from: &[TableRef]) {
        if self.schema.is_none() {
            return;
        }
        let binding = alias
            .map(str::to_string)
            .unwrap_or_else(|| table.last().to_string());
        let mut scope = vec![(binding, Some(table.clone()))];
        for t in from {
            collect(t, &mut scope);
        }
        self.stack.push(scope);
    }

    /// Enter an upsert's `DO UPDATE`: the table under its own name and as `excluded`.
    pub fn push_upsert(&mut self, table: &Name) {
        if self.schema.is_none() {
            return;
        }
        self.stack.push(vec![
            (table.last().to_string(), Some(table.clone())),
            ("excluded".to_string(), Some(table.clone())),
        ]);
    }

    /// The stated unique keys of a base table (see [`Schema::unique_keys`]).
    pub fn unique_keys(&self, table: &Name) -> Option<&'a [Vec<String>]> {
        self.schema?.unique_keys(table)
    }

    /// The complete column list of a base table (see [`Schema::complete_columns`]).
    pub fn complete_columns(&self, table: &Name) -> Option<Vec<&'a str>> {
        self.schema?.complete_columns(table)
    }

    pub fn pop(&mut self) {
        if self.schema.is_some() {
            self.stack.pop();
        }
    }

    /// True when `e` is a (possibly parenthesised) column the schema says is not text.
    pub fn is_non_text_column(&self, e: &Expr) -> bool {
        self.column(e)
            .is_some_and(|c| !matches!(c.class, ColumnClass::Text | ColumnClass::Unknown))
    }

    /// The collation a text column states when it is **not** bytewise (`en_US`, `NOCASE`,
    /// `utf8mb4_0900_ai_ci`, ...). `None` for a column whose collation is unstated: an unstated
    /// collation is unknown, never the database default.
    pub fn non_bytewise_collation(&self, e: &Expr) -> Option<String> {
        let c = self.column(e)?;
        match (&c.class, &c.collation) {
            (ColumnClass::Text, Some(name)) if !is_bytewise_collation(name) => Some(name.clone()),
            _ => None,
        }
    }

    /// The column a (possibly parenthesised) column reference names, when the schema knows it.
    fn column(&self, e: &Expr) -> Option<&Column> {
        let schema = self.schema?;
        match e {
            Expr::Nested(x) => self.column(x),
            Expr::Column(name) => self.column_of(schema, name),
            _ => None,
        }
    }

    fn column_of<'s>(&self, schema: &'s Schema, name: &Name) -> Option<&'s Column> {
        match name.0.as_slice() {
            [col] => {
                for scope in self.stack.iter().rev() {
                    let mut found: Vec<&'s Column> = Vec::new();
                    let mut unknown = false;
                    for (_, table) in scope {
                        match table {
                            Some(t) if schema.knows_table(t) => {
                                if let Some(c) = schema.get(t, col) {
                                    found.push(c);
                                }
                            }
                            // A derived table or a table the schema does not list may have the column.
                            _ => unknown = true,
                        }
                    }
                    match (found.first(), unknown) {
                        (Some(first), false) if found.iter().all(|c| c == first) => {
                            return Some(*first);
                        }
                        (None, false) => continue,
                        _ => return None,
                    }
                }
                None
            }
            [qual, col] => {
                for scope in self.stack.iter().rev() {
                    if let Some((_, table)) = scope.iter().find(|(b, _)| b == qual) {
                        return schema.get(table.as_ref()?, col);
                    }
                }
                None
            }
            [s, t, col] => schema.get(&Name(vec![s.clone(), t.clone()]), col),
            _ => None,
        }
    }
}

/// True for a collation that compares and orders text by bytes (code points): `C`, `POSIX`,
/// `ucs_basic`, SQLite's `BINARY`, MySQL/MariaDB `*_bin` and `binary`.
pub fn is_bytewise_collation(name: &str) -> bool {
    let n = name
        .trim()
        .trim_matches(['"', '`', '\''])
        .to_ascii_lowercase();
    matches!(n.as_str(), "c" | "posix" | "ucs_basic" | "binary") || n.ends_with("_bin")
}

fn collect(t: &TableRef, out: &mut Vec<(String, Option<Name>)>) {
    match t {
        TableRef::Table { name, alias } => out.push((
            alias.clone().unwrap_or_else(|| name.last().to_string()),
            Some(name.clone()),
        )),
        TableRef::Subquery { alias, .. } => out.push((alias.clone(), None)),
        TableRef::Function { name, alias, .. } => {
            out.push((alias.clone().unwrap_or_else(|| name.clone()), None))
        }
        TableRef::Join { left, right, .. } => {
            collect(left, out);
            collect(right, out);
        }
    }
}
