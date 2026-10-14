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

use std::collections::BTreeMap;

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

    pub fn with_collation(mut self, collation: impl Into<String>) -> Self {
        self.collation = Some(collation.into());
        self
    }
}

/// Base tables and their columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schema {
    tables: BTreeMap<Vec<String>, BTreeMap<String, Column>>,
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

    pub fn pop(&mut self) {
        if self.schema.is_some() {
            self.stack.pop();
        }
    }

    /// True when `e` is a (possibly parenthesised) column the schema says is not text.
    pub fn is_non_text_column(&self, e: &Expr) -> bool {
        let Some(schema) = self.schema else {
            return false;
        };
        match e {
            Expr::Nested(x) => self.is_non_text_column(x),
            Expr::Column(name) => self
                .class_of(schema, name)
                .is_some_and(|c| c != ColumnClass::Text),
            _ => false,
        }
    }

    fn class_of(&self, schema: &Schema, name: &Name) -> Option<ColumnClass> {
        match name.0.as_slice() {
            [col] => {
                for scope in self.stack.iter().rev() {
                    let mut found: Vec<ColumnClass> = Vec::new();
                    let mut unknown = false;
                    for (_, table) in scope {
                        match table {
                            Some(t) if schema.knows_table(t) => {
                                if let Some(c) = schema.get(t, col) {
                                    found.push(c.class);
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
                        return schema.get(table.as_ref()?, col).map(|c| c.class);
                    }
                }
                None
            }
            [s, t, col] => schema
                .get(&Name(vec![s.clone(), t.clone()]), col)
                .map(|c| c.class),
            _ => None,
        }
    }
}

fn collect(t: &TableRef, out: &mut Vec<(String, Option<Name>)>) {
    match t {
        TableRef::Table { name, alias } => out.push((
            alias.clone().unwrap_or_else(|| name.last().to_string()),
            Some(name.clone()),
        )),
        TableRef::Subquery { alias, .. } => out.push((alias.clone(), None)),
        TableRef::Join { left, right, .. } => {
            collect(left, out);
            collect(right, out);
        }
    }
}
