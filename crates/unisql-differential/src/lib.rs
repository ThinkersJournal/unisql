//! Differential harness. The only layer of UniSQL that tests *meaning* (DESIGN.md section 2.5).
//!
//! For each case: read the PostgreSQL text, translate it, run the **original on a real PostgreSQL**
//! and the **translation on a real SQLite**, over the same fixture data, and compare result sets
//! strictly (no int/float coercion; PostgreSQL booleans are read as 0/1, which is exactly the
//! `boolean_as_integer` allowance). A case that cannot connect **fails**; nothing is skipped.

use std::collections::BTreeSet;

use postgres::types::{ToSql, Type};
use postgres::{Client, NoTls};
use rusqlite::types::ValueRef;
use unisql::{Allowance, Options, Tier};

/// One value in a canonical result cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl Cell {
    fn sort_key(&self) -> String {
        match self {
            Cell::Null => "0:".into(),
            Cell::Int(i) => format!("1:{i:030}"),
            Cell::Real(r) => format!("2:{r:+030.9}"),
            Cell::Text(t) => format!("3:{t}"),
        }
    }
}

fn cells_equal(a: &Cell, b: &Cell) -> bool {
    match (a, b) {
        (Cell::Real(x), Cell::Real(y)) => (x - y).abs() < 1e-9,
        _ => a == b,
    }
}

pub type Rows = Vec<Vec<Cell>>;

/// A bind value, converted to the engine's declared parameter type.
#[derive(Clone, Debug)]
pub enum Param {
    Null,
    Int(i64),
    Real(f64),
    Text(&'static str),
}

pub struct Engines {
    pub pg: Client,
    pub lite: rusqlite::Connection,
}

pub fn pg_url() -> String {
    std::env::var("UNISQL_PG_URL")
        .unwrap_or_else(|_| "host=127.0.0.1 port=55432 user=postgres password=unisql".to_string())
}

impl Engines {
    /// Connect to PostgreSQL (must be reachable; this fails loudly otherwise) and open an in-memory
    /// SQLite, then load the same fixture into both.
    pub fn open(pg_fixture: &str, lite_fixture: &str) -> Result<Engines, String> {
        let mut pg = Client::connect(&pg_url(), NoTls)
            .map_err(|e| format!("cannot reach PostgreSQL at `{}`: {e}", pg_url()))?;
        pg.batch_execute("DROP SCHEMA IF EXISTS uni CASCADE; CREATE SCHEMA uni; SET search_path = uni;")
            .map_err(|e| format!("pg schema: {e}"))?;
        pg.batch_execute(pg_fixture).map_err(|e| format!("pg fixture: {e}"))?;
        let lite = rusqlite::Connection::open_in_memory().map_err(|e| e.to_string())?;
        // LIKE case sensitivity is a connection setting on SQLite (allowance
        // `like_case_sensitivity_is_connection_setting`); PostgreSQL's LIKE is case-sensitive.
        lite.execute_batch("PRAGMA case_sensitive_like = ON;").map_err(|e| e.to_string())?;
        lite.execute_batch(lite_fixture).map_err(|e| format!("sqlite fixture: {e}"))?;
        Ok(Engines { pg, lite })
    }

    pub fn sqlite_version(&self) -> String {
        self.lite
            .query_row("select sqlite_version()", [], |r| r.get::<_, String>(0))
            .unwrap_or_default()
    }

    pub fn pg_version(&mut self) -> String {
        self.pg
            .query_one("show server_version", &[])
            .map(|r| r.get::<_, String>(0))
            .unwrap_or_default()
    }

    /// Run on PostgreSQL inside a transaction that is always rolled back; the statement's own result
    /// rows (SELECT or RETURNING) are returned, then `after` is run (still inside the transaction)
    /// to observe the resulting table state.
    pub fn run_pg(&mut self, sql: &str, params: &[Param], after: Option<&str>) -> Result<(Rows, Rows), String> {
        let mut tx = self.pg.transaction().map_err(|e| e.to_string())?;
        let out = (|| {
            let stmt = tx.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
            let owned = pg_params(stmt.params(), params)?;
            let refs: Vec<&(dyn ToSql + Sync)> = owned.iter().map(|b| b.as_ref()).collect();
            let rows = if stmt.columns().is_empty() {
                tx.execute(&stmt, &refs).map_err(|e| format!("execute: {e}"))?;
                Vec::new()
            } else {
                let rs = tx.query(&stmt, &refs).map_err(|e| format!("query: {e}"))?;
                rs.iter().map(pg_row).collect::<Result<Rows, String>>()?
            };
            let post = match after {
                None => Vec::new(),
                Some(q) => {
                    let rs = tx.query(q, &[]).map_err(|e| format!("after: {e}"))?;
                    rs.iter().map(pg_row).collect::<Result<Rows, String>>()?
                }
            };
            Ok((rows, post))
        })();
        let _ = tx.rollback();
        out
    }

    pub fn run_sqlite(&mut self, sql: &str, params: &[Param], after: Option<&str>) -> Result<(Rows, Rows), String> {
        let tx = self.lite.transaction().map_err(|e| e.to_string())?;
        let out = (|| {
            let mut stmt = tx.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
            let binds: Vec<rusqlite::types::Value> = params
                .iter()
                .map(|p| match p {
                    Param::Null => rusqlite::types::Value::Null,
                    Param::Int(i) => rusqlite::types::Value::Integer(*i),
                    Param::Real(r) => rusqlite::types::Value::Real(*r),
                    Param::Text(t) => rusqlite::types::Value::Text((*t).to_string()),
                })
                .collect();
            let ncols = stmt.column_count();
            let rows = if ncols == 0 {
                stmt.execute(rusqlite::params_from_iter(binds.iter())).map_err(|e| format!("execute: {e}"))?;
                Vec::new()
            } else {
                let mut out = Vec::new();
                let mut q = stmt.query(rusqlite::params_from_iter(binds.iter())).map_err(|e| format!("query: {e}"))?;
                while let Some(r) = q.next().map_err(|e| format!("row: {e}"))? {
                    out.push(sqlite_row(r, ncols)?);
                }
                out
            };
            let post = match after {
                None => Vec::new(),
                Some(q) => {
                    let mut st = tx.prepare(q).map_err(|e| format!("after: {e}"))?;
                    let n = st.column_count();
                    let mut rs = st.query([]).map_err(|e| e.to_string())?;
                    let mut out = Vec::new();
                    while let Some(r) = rs.next().map_err(|e| e.to_string())? {
                        out.push(sqlite_row(r, n)?);
                    }
                    out
                }
            };
            Ok((rows, post))
        })();
        let _ = tx.rollback();
        out
    }
}

fn pg_params(types: &[Type], params: &[Param]) -> Result<Vec<Box<dyn ToSql + Sync>>, String> {
    if types.len() != params.len() {
        return Err(format!("statement has {} parameters, case supplies {}", types.len(), params.len()));
    }
    let mut out: Vec<Box<dyn ToSql + Sync>> = Vec::new();
    for (t, p) in types.iter().zip(params) {
        let b: Box<dyn ToSql + Sync> = match (t.clone(), p) {
            (Type::INT2, Param::Int(i)) => Box::new(*i as i16),
            (Type::INT4, Param::Int(i)) => Box::new(*i as i32),
            (Type::INT8, Param::Int(i)) => Box::new(*i),
            (Type::INT2, Param::Null) => Box::new(None::<i16>),
            (Type::INT4, Param::Null) => Box::new(None::<i32>),
            (Type::INT8, Param::Null) => Box::new(None::<i64>),
            (Type::FLOAT8, Param::Real(r)) => Box::new(*r),
            (Type::FLOAT8, Param::Int(i)) => Box::new(*i as f64),
            (Type::FLOAT8, Param::Null) => Box::new(None::<f64>),
            (Type::TEXT | Type::VARCHAR | Type::UNKNOWN, Param::Text(s)) => Box::new((*s).to_string()),
            (Type::TEXT | Type::VARCHAR | Type::UNKNOWN, Param::Null) => Box::new(None::<String>),
            (t, p) => return Err(format!("harness cannot bind {p:?} to PostgreSQL type {t}")),
        };
        out.push(b);
    }
    Ok(out)
}

fn pg_row(r: &postgres::Row) -> Result<Vec<Cell>, String> {
    let mut out = Vec::new();
    for (i, c) in r.columns().iter().enumerate() {
        let t = c.type_().clone();
        let cell = match t {
            Type::INT2 => r.get::<_, Option<i16>>(i).map(|v| Cell::Int(v as i64)),
            Type::INT4 => r.get::<_, Option<i32>>(i).map(|v| Cell::Int(v as i64)),
            Type::INT8 => r.get::<_, Option<i64>>(i).map(Cell::Int),
            Type::FLOAT4 => r.get::<_, Option<f32>>(i).map(|v| Cell::Real(v as f64)),
            Type::FLOAT8 => r.get::<_, Option<f64>>(i).map(Cell::Real),
            Type::BOOL => r.get::<_, Option<bool>>(i).map(|v| Cell::Int(v as i64)),
            Type::TEXT | Type::VARCHAR | Type::NAME | Type::BPCHAR => r.get::<_, Option<String>>(i).map(Cell::Text),
            ref other => return Err(format!("harness cannot read PostgreSQL type {other} (cast it in the statement)")),
        };
        out.push(cell.unwrap_or(Cell::Null));
    }
    Ok(out)
}

fn sqlite_row(r: &rusqlite::Row, n: usize) -> Result<Vec<Cell>, String> {
    let mut out = Vec::new();
    for i in 0..n {
        out.push(match r.get_ref(i).map_err(|e| e.to_string())? {
            ValueRef::Null => Cell::Null,
            ValueRef::Integer(v) => Cell::Int(v),
            ValueRef::Real(v) => Cell::Real(v),
            ValueRef::Text(t) => Cell::Text(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(_) => return Err("harness does not read blobs".into()),
        });
    }
    Ok(out)
}

/// Compare result sets. `ordered` = the statement defines an order; otherwise rows are compared as
/// multisets.
pub fn rows_equal(a: &Rows, b: &Rows, ordered: bool) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (mut a, mut b) = (a.clone(), b.clone());
    if !ordered {
        let key = |r: &Vec<Cell>| r.iter().map(Cell::sort_key).collect::<Vec<_>>().join("|");
        a.sort_by_key(key);
        b.sort_by_key(key);
    }
    a.iter().zip(&b).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| cells_equal(p, q)))
}

/// What a case expects of the translation.
#[derive(Clone, Debug)]
pub enum Expect {
    /// Translates, and the two engines agree.
    Same,
    /// Translation is refused, naming this construct, and produces no text.
    Refused(&'static str),
    /// Refused because the named allowance is not granted (proves the allowance gate).
    NeedsAllowance(Allowance),
    /// Translates (with the allowance) but the engines **disagree**: proves the allowance is needed.
    Diverges,
    /// The reader refuses it: the construct is not in the IR (named).
    NotInIr(&'static str),
    /// The text is valid PostgreSQL (the engine runs it) but the parser behind the reader rejects it:
    /// a recorded parser limitation, not a property of the SQL.
    ParserGap,
}

pub struct Case {
    pub name: &'static str,
    pub sql: &'static str,
    pub params: Vec<Param>,
    pub allow: Vec<Allowance>,
    pub ordered: bool,
    /// A SELECT run after the statement, inside the same transaction, to compare table state.
    pub after: Option<&'static str>,
    pub expect: Expect,
}

#[derive(Debug)]
pub struct Outcome {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub translated: Option<String>,
    /// Construct -> worst tier recorded by the report, when it translated.
    pub constructs: Vec<(String, Tier)>,
    pub rows: usize,
}

pub fn run_case(e: &mut Engines, c: &Case) -> Outcome {
    let opts = c.allow.iter().fold(Options::default(), |o, a| o.with(*a));
    let mut out = Outcome { name: c.name, passed: false, detail: String::new(), translated: None, constructs: vec![], rows: 0 };
    let tr = unisql::translate(c.sql, "postgres", "sqlite", &opts);
    match (&c.expect, tr) {
        (Expect::Refused(construct), Err(unisql::Error::Untranslatable(u))) => {
            out.passed = u.construct == *construct || u.construct.starts_with(construct);
            out.detail = format!("refused `{}`: {}", u.construct, u.reason);
            if !out.passed {
                out.detail = format!("expected refusal of `{construct}`, got {}", out.detail);
            }
        }
        (Expect::NeedsAllowance(a), Err(unisql::Error::Untranslatable(u))) => {
            out.passed = u.reason.contains(a.name());
            out.detail = format!("refused without allowance: {}", u.reason);
        }
        (Expect::ParserGap, Err(unisql::Error::Read(unisql::ReadError::Syntax(msg)))) => {
            match e.run_pg(c.sql, &c.params, None) {
                Ok(_) => {
                    out.passed = true;
                    out.detail = format!("parser gap (PostgreSQL accepts it): {msg}");
                }
                Err(pe) => out.detail = format!("PostgreSQL also rejects it, so this is not a parser gap: {pe}"),
            }
        }
        (Expect::NotInIr(construct), Err(unisql::Error::Read(unisql::ReadError::NotInIr { construct: got, detail }))) => {
            out.passed = got == *construct;
            out.detail = format!("not in IR `{got}`: {detail}");
        }
        (Expect::Diverges, Ok(t)) => {
            out.translated = Some(t.sql.clone());
            let orig = e.run_pg(c.sql, &c.params, c.after);
            let lite = e.run_sqlite(&t.sql, &c.params, c.after);
            match (orig, lite) {
                (Err(pe), Ok(l)) => {
                    out.passed = true;
                    out.detail = format!("diverges: postgres errors ({pe}); sqlite returns {:?}", l.0);
                }
                (Ok(o), Ok(l)) if !rows_equal(&o.0, &l.0, c.ordered) || !rows_equal(&o.1, &l.1, c.ordered) => {
                    out.passed = true;
                    out.detail = format!("diverges: postgres {:?} vs sqlite {:?}", o.0, l.0);
                }
                (o, l) => out.detail = format!("expected a divergence but engines agree/failed alike: {:?} / {:?}", o.map(|x| x.0), l.map(|x| x.0)),
            }
        }
        (Expect::Refused(_) | Expect::NeedsAllowance(_) | Expect::NotInIr(_) | Expect::ParserGap, Ok(t)) => {
            out.detail = format!("expected a refusal but got text: {}", t.sql);
            out.translated = Some(t.sql);
        }
        (_, Err(err)) => out.detail = format!("translate failed: {err}"),
        (Expect::Same, Ok(t)) => {
            out.translated = Some(t.sql.clone());
            out.constructs = t.report.entries.iter().map(|x| (x.construct.clone(), x.tier)).collect();
            // Identity check: Postgres -> Postgres must give the same results as the original.
            let pg_self = unisql::translate(c.sql, "postgres", "postgres", &Options::default());
            let pg_text = match pg_self {
                Ok(p) => p.sql,
                Err(err) => {
                    out.detail = format!("postgres->postgres failed: {err}");
                    return out;
                }
            };
            let orig = e.run_pg(c.sql, &c.params, c.after);
            let ident = e.run_pg(&pg_text, &c.params, c.after);
            let lite = e.run_sqlite(&t.sql, &c.params, c.after);
            match (orig, ident, lite) {
                (Ok(o), Ok(i), Ok(l)) => {
                    out.rows = o.0.len();
                    if !rows_equal(&o.0, &i.0, c.ordered) || !rows_equal(&o.1, &i.1, c.ordered) {
                        out.detail = format!("postgres->postgres differs from original; text: {pg_text}");
                    } else if !rows_equal(&o.0, &l.0, c.ordered) {
                        out.detail = format!("RESULT DIFFERS\n  postgres: {:?}\n  sqlite:   {:?}\n  sql: {}", o.0, l.0, t.sql);
                    } else if !rows_equal(&o.1, &l.1, c.ordered) {
                        out.detail = format!("STATE DIFFERS\n  postgres: {:?}\n  sqlite:   {:?}\n  sql: {}", o.1, l.1, t.sql);
                    } else {
                        out.passed = true;
                        out.detail = "same".into();
                    }
                }
                (o, i, l) => out.detail = format!("engine error: pg={:?} pg2pg={:?} sqlite={:?}\n  sql: {}", o.err(), i.err(), l.err(), t.sql),
            }
        }
    }
    out
}

/// The fixture constructs a matrix: distinct construct ids across passing cases.
pub fn matrix_rows(outcomes: &[Outcome]) -> BTreeSet<(String, &'static str)> {
    let mut m = BTreeSet::new();
    for o in outcomes.iter().filter(|o| o.passed && o.translated.is_some()) {
        for (c, t) in &o.constructs {
            m.insert((c.clone(), t.name()));
        }
    }
    m
}
