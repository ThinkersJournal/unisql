// SPDX-License-Identifier: MIT OR Apache-2.0
//! The checks and translation behind UniSQL's compile-time macros (`docs/MACROS.md`).
//!
//! [`expand`] takes what a macro call states (source dialect, targets, allowances, needs assertions
//! and the statement text) and either returns the per-target text and parameter data to embed, or every
//! problem found. Nothing here touches `proc_macro`, the file system or the environment.

use std::collections::BTreeSet;

use unisql::ast::{self, InsertSource};
use unisql::{Allowance, Options, Schema};

pub use unisql::ParamBinding;
pub use unisql::{SNAPSHOT_FORMAT, Snapshot, SnapshotError};

/// What one macro call states.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    /// The dialect the statement is written in (`postgres`, `sqlite`, `mysql`, `mariadb`).
    pub source: String,
    /// The targets to translate for; at least one, no duplicates.
    pub targets: Vec<String>,
    /// Allowance names granted at the call site (`Allowance::name()` spelling).
    pub allow: Vec<String>,
    /// Construct ids the statement must use.
    pub requires: Vec<String>,
    /// Construct ids the statement must not use.
    pub forbids: Vec<String>,
    pub sql: String,
    /// What the caller knows about its tables (see `unisql::schema_from_ddl`); the macro crate reads it
    /// from a file, so this crate does no I/O. With a schema the translator stops treating comparisons
    /// on non-text columns as text, and [`SchemaChecker`] runs.
    pub schema: Option<Schema>,
}

/// One target's translation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub target: String,
    pub sql: String,
    pub param_order: Vec<u32>,
    pub param_bindings: Vec<ParamBinding>,
}

/// A statement that passed every check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    /// Number of distinct source parameters.
    pub params: u32,
    /// The statement's construct ids, sorted.
    pub constructs: Vec<String>,
    /// One entry per target, in the order the call named them.
    pub rendered: Vec<Rendered>,
}

/// What kind of check failed; a macro maps each to the argument it points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Source,
    Targets,
    Allow,
    Syntax,
    Placeholders,
    Refused,
    Requires,
    Forbids,
    /// A [`Checker`] found the statement wrong against the caller's tables.
    Catalog,
}

/// One failed check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub kind: Kind,
    pub message: String,
}

fn problem(kind: Kind, message: impl Into<String>) -> Problem {
    Problem {
        kind,
        message: message.into(),
    }
}

/// The `Line: n, Column: m` the parser puts at the end of a syntax error, 1-based.
fn parser_position(message: &str) -> Option<(usize, usize)> {
    let rest = message.rsplit_once("Line: ")?.1;
    let (line, rest) = rest.split_once(", Column: ")?;
    let column: String = rest.chars().take_while(char::is_ascii_digit).collect();
    Some((line.trim().parse().ok()?, column.parse().ok()?))
}

/// A syntax problem for `sql`: the reader's message and, when it gave a position, the offending line
/// with a caret under the column. The statement is one string literal, so a line number counts inside it.
fn syntax_problem(message: String, sql: &str) -> Problem {
    let Some((line, column)) = parser_position(&message) else {
        return problem(Kind::Syntax, message);
    };
    let Some(text) = sql.lines().nth(line.saturating_sub(1)) else {
        return problem(Kind::Syntax, message);
    };
    let caret = " ".repeat(column.saturating_sub(1).min(text.chars().count()));
    problem(
        Kind::Syntax,
        format!(
            "{message}
  {text}
  {caret}^"
        ),
    )
}

fn known_allowances() -> String {
    Allowance::ALL
        .iter()
        .map(|a| a.name())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_dialect(kind: Kind, role: &str, name: &str, out: &mut Vec<Problem>) -> bool {
    if unisql::dialects().contains(&name) {
        return true;
    }
    out.push(problem(
        kind,
        format!(
            "unknown {role} dialect `{name}` (known: {})",
            unisql::dialects().join(", ")
        ),
    ));
    false
}

/// What a [`Checker`] is told besides the statement. Non-exhaustive: more is added as checks need it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct CheckContext<'a> {
    /// The dialect the statement is written in.
    pub source: &'a str,
    pub schema: Option<&'a Schema>,
}

/// A check over the parsed source statement, run after syntax and before translation. It reports every
/// problem it finds as [`Kind::Catalog`]; it must only report what is certain (a name it cannot resolve
/// is unknown, not wrong). PersistAnt implements this over its catalog snapshot.
pub trait Checker {
    fn check(&self, stmt: &ast::Statement, ctx: &CheckContext<'_>) -> Vec<Problem>;
}

/// Checks that need only names: an `INSERT` row whose width differs from its column list and a column
/// listed twice (both from the statement alone), and, where the schema states a table's column list in
/// full, a column the table does not have and a row of the wrong width for a statement without a list. `INSERT` and
/// `UPDATE .. SET` only; other statements are not looked at.
pub struct SchemaChecker;

fn catalog(message: String) -> Problem {
    Problem {
        kind: Kind::Catalog,
        message,
    }
}

/// A column unknown to a complete column list. Compared case-insensitively so an unquoted name written
/// in another case is never reported: only a name with no match at all is.
fn unknown_column(complete: &[&str], column: &str) -> bool {
    !complete.iter().any(|c| c.eq_ignore_ascii_case(column))
}

impl Checker for SchemaChecker {
    fn check(&self, stmt: &ast::Statement, ctx: &CheckContext<'_>) -> Vec<Problem> {
        let mut out = Vec::new();
        match stmt {
            ast::Statement::Insert(i) => {
                let complete = ctx.schema.and_then(|s| s.complete_columns(&i.table));
                let table = i.table.last();
                let mut seen = Vec::new();
                for c in &i.columns {
                    if seen.iter().any(|s: &&String| s.eq_ignore_ascii_case(c)) {
                        out.push(catalog(format!(
                            "column `{c}` is listed twice in the INSERT into `{table}`"
                        )));
                    }
                    seen.push(c);
                    if complete
                        .as_deref()
                        .is_some_and(|cols| unknown_column(cols, c))
                    {
                        out.push(catalog(format!("table `{table}` has no column `{c}`")));
                    }
                }
                if let InsertSource::Values(rows) = &i.source {
                    let width = if i.columns.is_empty() {
                        complete.as_ref().map(Vec::len)
                    } else {
                        Some(i.columns.len())
                    };
                    if let Some(width) = width {
                        for (n, row) in rows.iter().enumerate() {
                            if row.len() != width {
                                out.push(catalog(format!(
                                    "VALUES row {} has {} value(s) but the INSERT into `{table}` names {width} column(s)",
                                    n + 1,
                                    row.len()
                                )));
                            }
                        }
                    }
                }
            }
            ast::Statement::Update(u) => {
                if let Some(cols) = ctx.schema.and_then(|s| s.complete_columns(&u.table)) {
                    for a in &u.set {
                        if unknown_column(&cols, &a.column) {
                            out.push(catalog(format!(
                                "table `{}` has no column `{}`",
                                u.table.last(),
                                a.column
                            )));
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }
}

/// Check and translate one call. `Err` holds every problem found, never an empty list.
pub fn expand(input: &Input) -> Result<Expansion, Vec<Problem>> {
    expand_with(input, &[])
}

/// As [`expand`], also running `checkers` over the parsed statement (after the syntax check, before
/// translation). [`SchemaChecker`] runs first whenever `input.schema` is set. Every catalog problem is
/// reported, not the first; a statement with catalog problems is not translated.
pub fn expand_with(input: &Input, checkers: &[&dyn Checker]) -> Result<Expansion, Vec<Problem>> {
    let mut problems = Vec::new();

    check_dialect(Kind::Source, "source", &input.source, &mut problems);
    if input.targets.is_empty() {
        problems.push(problem(
            Kind::Targets,
            "no targets named: write `targets = [..]` (a call translates for exactly the targets it lists)",
        ));
    }
    let mut seen = BTreeSet::new();
    for t in &input.targets {
        check_dialect(Kind::Targets, "target", t, &mut problems);
        if !seen.insert(t.as_str()) {
            problems.push(problem(
                Kind::Targets,
                format!("target `{t}` is named twice"),
            ));
        }
    }
    let mut opts = Options::default();
    if let Some(schema) = &input.schema {
        opts = opts.with_schema(schema.clone());
    }
    for name in &input.allow {
        match Allowance::ALL.iter().find(|a| a.name() == name) {
            Some(a) => opts = opts.with(*a),
            None => problems.push(problem(
                Kind::Allow,
                format!("unknown allowance `{name}` (known: {})", known_allowances()),
            )),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    let analysis = match unisql::analyse_with(&input.sql, &input.source, &opts) {
        Ok(a) => a,
        Err(e) => return Err(vec![syntax_problem(e.to_string(), &input.sql)]),
    };
    if let Some(construct) = &analysis.unreadable {
        return Err(vec![problem(
            Kind::Syntax,
            format!("construct `{construct}` is not in the IR, so it cannot be translated"),
        )]);
    }

    let stmt = match unisql::read_statement(&input.sql, &input.source) {
        Ok(stmt) => stmt,
        Err(e) => return Err(vec![syntax_problem(e.to_string(), &input.sql)]),
    };
    let ctx = CheckContext {
        source: &input.source,
        schema: input.schema.as_ref(),
    };
    let mut found: Vec<Problem> = SchemaChecker.check(&stmt, &ctx);
    for checker in checkers {
        found.extend(checker.check(&stmt, &ctx));
    }
    if !found.is_empty() {
        return Err(found);
    }

    let mut rendered = Vec::new();
    let mut params = 0;
    let mut gap_reported = false;
    for t in &input.targets {
        match unisql::translate(&input.sql, &input.source, t, &opts) {
            Ok(tr) => {
                params = tr.param_count;
                let distinct: BTreeSet<u32> = tr.param_order.iter().copied().collect();
                if distinct.len() as u32 != tr.param_count && !gap_reported {
                    gap_reported = true;
                    problems.push(problem(
                        Kind::Placeholders,
                        format!(
                            "placeholders must be `$1..$n` with none skipped: the highest is ${} but only {} distinct are used",
                            tr.param_count,
                            distinct.len()
                        ),
                    ));
                }
                rendered.push(Rendered {
                    target: t.clone(),
                    sql: tr.sql,
                    param_order: tr.param_order,
                    param_bindings: tr.param_bindings,
                });
            }
            Err(e) => problems.push(problem(Kind::Refused, format!("for target `{t}`: {e}"))),
        }
    }

    for id in &input.requires {
        if !analysis.constructs.contains(id) {
            problems.push(problem(
                Kind::Requires,
                format!("the statement does not use the required construct `{id}`"),
            ));
        }
    }
    for id in &input.forbids {
        if analysis.constructs.contains(id) {
            problems.push(problem(
                Kind::Forbids,
                format!("the statement uses the forbidden construct `{id}`"),
            ));
        }
    }

    if problems.is_empty() {
        Ok(Expansion {
            params,
            constructs: analysis.constructs.into_iter().collect(),
            rendered,
        })
    } else {
        Err(problems)
    }
}
