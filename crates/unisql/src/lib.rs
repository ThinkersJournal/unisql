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

pub use unisql_core::ast;
pub use unisql_core::{
    Allowance, Entry, Options, ReadError, Report, Target, Tier, Translation, Untranslatable,
};
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
            Error::UnknownDialect(d) => write!(f, "unknown dialect `{d}` (known: {})", dialects().join(", ")),
            Error::NoReader(d) => write!(f, "dialect `{d}` cannot be used as a source yet"),
            Error::Read(e) => write!(f, "{e}"),
            Error::Untranslatable(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for Error {}

/// Names of the dialects compiled in.
pub fn dialects() -> &'static [&'static str] {
    &["postgres", "sqlite"]
}

fn read(sql: &str, source: &str) -> Result<ast::Statement, Error> {
    match source {
        "postgres" => unisql_dialect_postgres::read(sql).map_err(Error::Read),
        "sqlite" => Err(Error::NoReader(source.to_string())),
        other => Err(Error::UnknownDialect(other.to_string())),
    }
}

fn target(name: &str) -> Result<Box<dyn Target>, Error> {
    match name {
        "postgres" => Ok(Box::new(Postgres)),
        "sqlite" => Ok(Box::new(Sqlite)),
        other => Err(Error::UnknownDialect(other.to_string())),
    }
}

/// Translate one statement from `source` to `target`.
pub fn translate(sql: &str, source: &str, target_dialect: &str, opts: &Options) -> Result<Translation, Error> {
    let tgt = target(target_dialect)?;
    let stmt = read(sql, source)?;
    unisql_core::write(&stmt, source, tgt.as_ref(), opts).map_err(Error::Untranslatable)
}

/// Like [`translate`], returning only the text. Still errors on refusal.
pub fn translate_str(sql: &str, source: &str, target_dialect: &str, opts: &Options) -> Result<String, Error> {
    translate(sql, source, target_dialect, opts).map(|t| t.sql)
}
