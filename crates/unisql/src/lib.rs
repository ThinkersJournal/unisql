// SPDX-License-Identifier: MIT OR Apache-2.0
//! UniSQL: translate SQL text from a source dialect to a target dialect.
//!
//! ```
//! use unisql::{translate, Options};
//! let t = translate("SELECT id FROM users WHERE id = $1", "postgres", "sqlite", &Options::default())
//!     .unwrap();
//! assert_eq!(t.sql, r#"SELECT "id" FROM "users" WHERE "id" = ?1"#);
//! ```
//!
//! No I/O, no driver, nothing about sqlx, Diesel or PersistAnt: integrations are separate crates
//! that call [`translate`]. A construct the target cannot give with the source's meaning returns
//! [`Error::Untranslatable`] and **no text**; a construct that is only the same under an assumption
//! needs that assumption named in [`Options`] (see [`Allowance`]).

use std::fmt;

// Compiles the README examples as doc-tests, so a change to the API that breaks them fails `cargo test`.
#[cfg(doctest)]
mod readme_doctests {
    #[doc = include_str!("../README.md")]
    struct CrateReadme;
    #[doc = include_str!("../../../README.md")]
    struct RootReadme;
}

pub use unisql_core::ast;
#[cfg(feature = "serde")]
pub use unisql_core::snapshot::{SNAPSHOT_FORMAT, Snapshot, SnapshotError};
pub use unisql_core::{
    Allowance, Column, ColumnClass, Entry, Options, ParamBinding, ReadError, Report, Schema,
    TableView, Target, Tier, Translation, Untranslatable, is_bytewise_collation,
};
use unisql_dialect_mysql::Mysql;
use unisql_dialect_postgres::Postgres;
use unisql_dialect_sqlite::Sqlite;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The dialect name is not compiled in.
    UnknownDialect(String),
    /// The dialect exists but has no reader yet (it is target-only).
    NoReader(String),
    /// The text is not valid for the source dialect, or uses a construct the IR does not represent.
    Read(ReadError),
    /// The target cannot give the source's meaning, or a needed allowance was not granted.
    Untranslatable(Untranslatable),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownDialect(d) => write!(
                f,
                "unknown dialect `{d}` (known: {})",
                dialects().join(", ")
            ),
            Error::NoReader(d) => write!(f, "dialect `{d}` cannot be used as a source yet"),
            Error::Read(e) => write!(f, "{e}"),
            Error::Untranslatable(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for Error {}

/// Names of the dialects compiled in.
pub fn dialects() -> &'static [&'static str] {
    &["postgres", "sqlite", "mysql", "mariadb"]
}

fn read(sql: &str, source: &str) -> Result<unisql_core::Read, Error> {
    match source {
        "postgres" => unisql_dialect_postgres::read(sql).map_err(Error::Read),
        "sqlite" => unisql_dialect_sqlite::read(sql).map_err(Error::Read),
        "mysql" | "mariadb" => Mysql::read(sql).map_err(Error::Read),
        other => Err(Error::UnknownDialect(other.to_string())),
    }
}

/// Read one statement written in `source` into the IR, without translating it: what a checker that
/// looks at the statement itself (`unisql-macro-core::Checker`) works from.
pub fn read_statement(sql: &str, source: &str) -> Result<ast::Statement, Error> {
    read(sql, source).map(|r| r.stmt)
}

fn target(name: &str) -> Result<Box<dyn Target>, Error> {
    match name {
        "postgres" => Ok(Box::new(Postgres)),
        "sqlite" => Ok(Box::new(Sqlite)),
        "mysql" => Ok(Box::new(Mysql::for_mysql())),
        "mariadb" => Ok(Box::new(Mysql::for_mariadb())),
        other => Err(Error::UnknownDialect(other.to_string())),
    }
}

/// A [`Schema`] read out of DDL text, and what could not be read.
#[derive(Clone, Debug, Default)]
pub struct DdlSchema {
    pub schema: Schema,
    /// Statements skipped because the parser could not read them (head and reason). A skipped
    /// statement can only leave columns unregistered, which the translator treats as unknown.
    pub skipped: Vec<String>,
}

/// Fill a [`Schema`] from a DDL script (for example a project's `migrations/*.sql` concatenated, in
/// order) written in `dialect`. Reading only: DDL is not translated and nothing is written out.
/// `CREATE TABLE`, `ALTER TABLE .. ADD|DROP|ALTER COLUMN .. TYPE` `CREATE VIEW` (its columns, where every one can be named) and `DROP TABLE`/`DROP VIEW` are applied in
/// order; table names are kept exactly as written (see [`Schema`]). A column whose declared type is not
/// classified is registered as unknown, never omitted. A column's collation is only what the DDL states
/// on the column (`COLLATE x`): no collation is guessed from the database default.
pub fn schema_from_ddl(ddl: &str, dialect: &str) -> Result<DdlSchema, Error> {
    use unisql_sqlparser_adapter as ad;
    let rules = match dialect {
        "postgres" => &ad::POSTGRES,
        "sqlite" => &ad::SQLITE,
        "mysql" | "mariadb" => &ad::MYSQL,
        other => return Err(Error::UnknownDialect(other.to_string())),
    };
    let r = ad::read_schema(ddl, rules).map_err(Error::Read)?;
    Ok(DdlSchema {
        schema: r.schema,
        skipped: r.skipped,
    })
}

/// Translate one statement from `source` to `target`.
pub fn translate(
    sql: &str,
    source: &str,
    target_dialect: &str,
    opts: &Options,
) -> Result<Translation, Error> {
    let tgt = target(target_dialect)?;
    let read = read(sql, source)?;
    // Assumptions the source text itself carries go through the same gate as target-side ones.
    for n in &read.notes {
        if source != target_dialect && !opts.allow.contains(&n.allowance) {
            return Err(Error::Untranslatable(Untranslatable {
                construct: n.construct.clone(),
                source: source.to_string(),
                target: target_dialect.to_string(),
                reason: format!(
                    "conditional: needs allowance `{}`: {}",
                    n.allowance.name(),
                    n.note
                ),
                alternatives: vec![],
            }));
        }
    }
    let mut t = unisql_core::write(&read.stmt, source, tgt.as_ref(), opts)
        .map_err(Error::Untranslatable)?;
    for n in read.notes {
        t.report.push(Entry {
            construct: n.construct,
            tier: Tier::Conditional,
            note: n.note,
            allowance: Some(n.allowance),
        });
    }
    Ok(t)
}

/// Like [`translate`], returning only the text. Still errors on refusal.
pub fn translate_str(
    sql: &str,
    source: &str,
    target_dialect: &str,
    opts: &Options,
) -> Result<String, Error> {
    translate(sql, source, target_dialect, opts).map(|t| t.sql)
}

/// What a statement needs, independent of any target.
///
/// `constructs` are the construct ids the statement uses (the same strings a [`Report`] and the
/// matrix files use), read from the IR alone: no target contributes to this set, so a test can
/// check it against any target's report. `refused` holds the leaf id (`cast.to_uuid`, `fn:md5`) of every form that **some** registered
/// target refuses; every id in it is also in `constructs`, so a gate that reads only `constructs`
/// cannot let a refused form through. It is "refused anywhere", with every allowance granted. A target stops at its *first*
/// refusal, so a statement with two refused forms contributes one per target. `unreadable` names the
/// construct the IR does not represent, in which case `constructs` is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    pub constructs: std::collections::BTreeSet<String>,
    pub refused: std::collections::BTreeSet<String>,
    pub unreadable: Option<String>,
    pub param_count: u32,
}

/// Extract a statement's needs. `Err` only for text the source dialect cannot parse, or an unknown
/// source dialect; a construct outside the IR is reported in [`Analysis::unreadable`].
pub fn analyse(sql: &str, source: &str) -> Result<Analysis, Error> {
    analyse_with(sql, source, &Options::default())
}

/// As [`analyse`], with the caller's [`Options`]: a [`Schema`] in them makes a comparison on a column
/// that is not text name no `text_comparison`, as `translate` with the same options records none.
/// (The allowances in `opts` are not used: `analyse` always asks what every allowance would permit.)
pub fn analyse_with(sql: &str, source: &str, opts: &Options) -> Result<Analysis, Error> {
    let read = match read(sql, source) {
        Ok(r) => r,
        Err(Error::Read(ReadError::NotInIr { construct, .. })) => {
            return Ok(Analysis {
                unreadable: Some(construct),
                ..Analysis::default()
            });
        }
        Err(e) => return Err(e),
    };
    let mut a = Analysis {
        constructs: unisql_core::needs::constructs_with(&read.stmt, opts.schema.as_ref()),
        ..Analysis::default()
    };
    for n in &read.notes {
        a.constructs.insert(n.construct.clone());
    }
    let all = Allowance::ALL
        .iter()
        .fold(opts.clone(), |o, al| o.with(*al));
    for name in dialects() {
        let Ok(tgt) = target(name) else { continue };
        match unisql_core::write(&read.stmt, source, tgt.as_ref(), &all) {
            Ok(t) => {
                a.param_count = a.param_count.max(t.param_count);
            }
            Err(u) => {
                a.refused.insert(u.subject());
            }
        }
    }
    Ok(a)
}
