// SPDX-License-Identifier: MIT OR Apache-2.0
//! What a UniSQL compile-time macro expands to (`docs/MACROS.md` section 3).
//!
//! A proc-macro crate cannot export types, so the value lives here. Everything is `const`-constructible
//! from `&'static` data; nothing is parsed or allocated at run time.
#![no_std]

/// A database a statement was translated for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Postgres,
    Sqlite,
    Mysql,
    Mariadb,
}

impl Target {
    /// The dialect name the macros and `unisql` use (`"postgres"`, `"sqlite"`, `"mysql"`, `"mariadb"`).
    pub const fn name(self) -> &'static str {
        match self {
            Target::Postgres => "postgres",
            Target::Sqlite => "sqlite",
            Target::Mysql => "mysql",
            Target::Mariadb => "mariadb",
        }
    }
}

/// How one source parameter is bound on a target (mirror of `unisql::ParamBinding`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    /// A single value.
    Scalar,
    /// A native array value.
    NativeArray,
    /// A JSON array bound as text.
    JsonArrayText,
}

/// One statement's text and parameter data for one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub target: Target,
    pub sql: &'static str,
    /// Source parameter numbers in the order their placeholders appear in `sql`.
    pub param_order: &'static [u32],
    /// One entry per source parameter (index `n - 1` is `$n`).
    pub param_bindings: &'static [Binding],
}

/// A statement checked and translated at build time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Query {
    /// Number of distinct source parameters (`$1..$n`).
    pub params: u32,
    /// The construct ids the statement uses (sorted).
    pub constructs: &'static [&'static str],
    /// One entry per target the call named.
    pub rendered: &'static [Rendered],
}

impl Query {
    /// The text and parameter data for `target`, or `None` when the call did not name it.
    pub const fn for_target(&self, target: Target) -> Option<&Rendered> {
        let mut i = 0;
        while i < self.rendered.len() {
            if self.rendered[i].target as u8 == target as u8 {
                return Some(&self.rendered[i]);
            }
            i += 1;
        }
        None
    }

    /// Whether the statement uses the construct `id`.
    pub const fn uses(&self, id: &str) -> bool {
        let mut i = 0;
        while i < self.constructs.len() {
            if str_eq(self.constructs[i], id) {
                return true;
            }
            i += 1;
        }
        false
    }
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: Query = Query {
        params: 1,
        constructs: &["ordered_comparison", "upsert"],
        rendered: &[
            Rendered {
                target: Target::Sqlite,
                sql: "SELECT ?1",
                param_order: &[1],
                param_bindings: &[Binding::Scalar],
            },
            Rendered {
                target: Target::Mysql,
                sql: "SELECT ?",
                param_order: &[1],
                param_bindings: &[Binding::Scalar],
            },
        ],
    };

    // Evaluated at compile time: a wrong answer fails the build, not just the test.
    const SQLITE_TEXT: &str = match Q.for_target(Target::Sqlite) {
        Some(r) => r.sql,
        None => "",
    };
    const _: () = assert!(Q.uses("upsert"));

    #[test]
    fn lookup_is_const_and_finds_only_named_targets() {
        assert_eq!(SQLITE_TEXT, "SELECT ?1");
        assert_eq!(Q.for_target(Target::Mysql).unwrap().sql, "SELECT ?");
        assert!(Q.for_target(Target::Postgres).is_none());
        assert!(Q.for_target(Target::Mariadb).is_none());
    }

    #[test]
    fn uses_matches_whole_ids_only() {
        assert!(!Q.uses("upser"));
        assert!(!Q.uses("upserts"));
        assert!(!Q.uses(""));
    }

    #[test]
    fn target_names_are_the_dialect_names() {
        assert_eq!(Target::Mariadb.name(), "mariadb");
        assert_eq!(Target::Postgres.name(), "postgres");
    }
}
