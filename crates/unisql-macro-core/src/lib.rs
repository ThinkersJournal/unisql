// SPDX-License-Identifier: MIT OR Apache-2.0
//! The checks and translation behind UniSQL's compile-time macros (`docs/MACROS.md`).
//!
//! [`expand`] takes what a macro call states (source dialect, targets, allowances, needs assertions
//! and the statement text) and either returns the per-target text and parameter data to embed, or every
//! problem found. Nothing here touches `proc_macro`, the file system or the environment.

use std::collections::BTreeSet;

use unisql::{Allowance, Error, Options, ParamBinding};

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

/// Check and translate one call. `Err` holds every problem found, never an empty list.
pub fn expand(input: &Input) -> Result<Expansion, Vec<Problem>> {
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
        Err(Error::Read(e)) => return Err(vec![problem(Kind::Syntax, e.to_string())]),
        Err(e) => return Err(vec![problem(Kind::Syntax, e.to_string())]),
    };
    if let Some(construct) = &analysis.unreadable {
        return Err(vec![problem(
            Kind::Syntax,
            format!("construct `{construct}` is not in the IR, so it cannot be translated"),
        )]);
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
