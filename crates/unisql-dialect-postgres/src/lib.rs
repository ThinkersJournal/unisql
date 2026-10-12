//! PostgreSQL dialect module: a reader (text -> IR) and a writer ([`Postgres`] target).

mod reader;

pub use reader::read;
use unisql_core::Target;

/// The PostgreSQL target. Every hook keeps the core's defaults, which describe PostgreSQL.
#[derive(Clone, Copy, Debug, Default)]
pub struct Postgres;

impl Target for Postgres {
    fn name(&self) -> &'static str {
        "postgres"
    }
}
