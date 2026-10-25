// SPDX-License-Identifier: MIT OR Apache-2.0
//! The MySQL-source corpus: real DML of a MySQL-backed project plus a curated list of MySQL/MariaDB
//! idioms (`corpus/mysql/`), run through the real translator and against the real engines.
//!
//! Per statement it records (1) whether MySQL 8.4 and MariaDB 11.4 accept it (PREPARE over the
//! corpus schema: this keeps the corpus honest), (2) the translator's verdict to PostgreSQL,
//! SQLite and MariaDB with every allowance granted and the corpus schema read from its DDL, and
//! (3) whether the PostgreSQL / SQLite engine accepts the translated text over an equivalent
//! schema. The result is compared with `corpus/mysql/expected.tsv`; `UNISQL_BLESS=1` rewrites it.
//! The gap list in ROADMAP.md is read from that file. Fails loudly if an engine is unreachable.

use std::fmt::Write as _;

use unisql::{Allowance, Error, Options, Tier};
use unisql_differential::Engines;

const SCHEMA: &str = include_str!("../corpus/mysql/schema.sql");
const CURATED: &str = include_str!("../corpus/mysql/curated.sql");
const AUTH: &str = include_str!("../corpus/mysql/auth_framework.sql");
const EXPECTED_PATH: &str = "corpus/mysql/expected.tsv";

const PG: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
  active integer NOT NULL DEFAULT 1, score double precision, city text, created_at timestamp, meta jsonb);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL, amount integer NOT NULL,
  status text NOT NULL, created_at timestamp);
CREATE TABLE kv (k text PRIMARY KEY, v text, n integer);
CREATE TABLE events (id integer PRIMARY KEY, at timestamp NOT NULL);
CREATE TABLE auth_tokens (token_id text PRIMARY KEY, user_id text NOT NULL, access_token text NOT NULL,
  refresh_token text, token_type text, expires_at timestamp NOT NULL, scopes text, issued_at timestamp NOT NULL,
  auth_method text NOT NULL, subject text, issuer text, client_id text, metadata jsonb, created_at timestamp);
CREATE TABLE sessions (session_id text PRIMARY KEY, user_id text NOT NULL, data jsonb NOT NULL,
  expires_at timestamp, created_at timestamp, last_activity timestamp, ip_address text, user_agent text);
CREATE TABLE kv_store ("key" text PRIMARY KEY, value bytea NOT NULL, expires_at timestamp, created_at timestamp);
"#;

const LITE: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
  active integer NOT NULL DEFAULT 1, score real, city text, created_at text, meta text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL, amount integer NOT NULL,
  status text NOT NULL, created_at text);
CREATE TABLE kv (k text PRIMARY KEY, v text, n integer);
CREATE TABLE events (id integer PRIMARY KEY, at text NOT NULL);
CREATE TABLE auth_tokens (token_id text PRIMARY KEY, user_id text NOT NULL, access_token text NOT NULL,
  refresh_token text, token_type text, expires_at text NOT NULL, scopes text, issued_at text NOT NULL,
  auth_method text NOT NULL, subject text, issuer text, client_id text, metadata text, created_at text);
CREATE TABLE sessions (session_id text PRIMARY KEY, user_id text NOT NULL, data text NOT NULL,
  expires_at text, created_at text, last_activity text, ip_address text, user_agent text);
CREATE TABLE kv_store ("key" text PRIMARY KEY, value blob NOT NULL, expires_at text, created_at text);
"#;

struct Stmt {
    id: String,
    group: String,
    origin: String,
    sql: String,
}

/// Blocks separated by blank lines; a block is `-- <id> <group> [origin]` then the SQL. A block
/// with no SQL line (file preamble) is skipped.
fn parse(text: &str, default_origin: &str) -> Vec<Stmt> {
    let mut out = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let lines: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty()).collect();
        let Some(head) = lines.first().and_then(|l| l.strip_prefix("-- ")) else {
            continue;
        };
        let sql: Vec<&str> = lines[1..]
            .iter()
            .copied()
            .filter(|l| !l.starts_with("--"))
            .collect();
        let toks: Vec<&str> = head.split_whitespace().collect();
        if sql.is_empty() || toks.len() < 2 || head.contains("SPDX") {
            continue;
        }
        out.push(Stmt {
            id: toks[0].to_string(),
            group: toks[1].to_string(),
            origin: toks.get(2).copied().unwrap_or(default_origin).to_string(),
            sql: sql.join("\n"),
        });
    }
    out
}

fn corpus() -> Vec<Stmt> {
    let mut v = parse(CURATED, "curated");
    v.extend(parse(AUTH, "auth-framework"));
    v
}

fn verdict(r: Result<unisql::Translation, Error>) -> (String, Option<String>) {
    match r {
        Ok(t) => {
            let needed: std::collections::BTreeSet<&str> = t
                .report
                .entries
                .iter()
                .filter(|e| e.tier == Tier::Conditional)
                .filter_map(|e| e.allowance.map(|a| a.name()))
                .collect();
            let s = if needed.is_empty() {
                "translated".to_string()
            } else {
                format!(
                    "conditional:{}",
                    needed.into_iter().collect::<Vec<_>>().join("+")
                )
            };
            (s, Some(t.sql))
        }
        Err(Error::Untranslatable(u)) => (format!("refused:{}", u.construct), None),
        Err(Error::Read(unisql::ReadError::NotInIr { construct, .. })) => {
            (format!("not_in_ir:{construct}"), None)
        }
        Err(Error::Read(unisql::ReadError::Syntax(_))) => ("syntax".to_string(), None),
        Err(e) => (format!("error:{e}"), None),
    }
}

#[test]
fn mysql_source_corpus() {
    use mysql::prelude::Queryable;
    let stmts = corpus();
    assert!(stmts.len() >= 100, "corpus has {} statements", stmts.len());
    let ids: std::collections::BTreeSet<_> = stmts.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids.len(), stmts.len(), "statement ids are unique");

    // Validity on the two source engines.
    let mut valid: Vec<[&'static str; 2]> = vec![["?", "?"]; stmts.len()];
    for (k, maria) in [false, true].into_iter().enumerate() {
        // a checkout with CRLF line endings must still split on a semicolon and newline
        let schema = SCHEMA.replace("\r\n", "\n");
        let mut e = Engines::open_mysql_with("", "", &schema, maria, true)
            .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
        let conn = e.mysql.as_mut().unwrap();
        for (i, s) in stmts.iter().enumerate() {
            valid[i][k] = if conn.prep(&s.sql).is_ok() {
                "ok"
            } else {
                "rejected"
            };
        }
    }
    for (s, v) in stmts.iter().zip(&valid) {
        assert!(
            v.contains(&"ok"),
            "corpus statement `{}` is accepted by neither MySQL nor MariaDB: {}",
            s.id,
            s.sql
        );
    }

    // The translator, then the target engines over equivalent schemas.
    let schema = unisql::schema_from_ddl(SCHEMA, "mysql").expect("corpus DDL reads");
    let opts = Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
        .with_schema(schema.schema);
    let mut e = Engines::open(PG, LITE)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    let mut tsv =
        String::from("id\tgroup\torigin\tmysql\tmariadb\tpostgres\tsqlite\tmariadb_target\n");
    for (s, v) in stmts.iter().zip(&valid) {
        let mut cols = Vec::new();
        for target in ["postgres", "sqlite", "mariadb"] {
            let (mut v, sql) = verdict(unisql::translate(&s.sql, "mysql", target, &opts));
            if let Some(sql) = sql {
                // the mariadb target column is judged by translation alone here, as before
                let ok = match target {
                    "mariadb" => Ok(()),
                    _ => e.prepares(target, &sql),
                };
                if ok.is_err() {
                    v = format!("invalid_output|{v}");
                }
            }
            cols.push(v);
        }
        writeln!(
            tsv,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            s.id, s.group, s.origin, v[0], v[1], cols[0], cols[1], cols[2]
        )
        .unwrap();
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(EXPECTED_PATH);
    if std::env::var("UNISQL_BLESS").is_ok() {
        std::fs::write(&path, &tsv).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        want == tsv,
        "corpus verdicts changed; review the diff and rerun with UNISQL_BLESS=1\n--- got ---\n{tsv}"
    );
}
