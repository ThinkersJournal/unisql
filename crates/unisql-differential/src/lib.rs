// SPDX-License-Identifier: MIT OR Apache-2.0
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
use unisql::{Allowance, Options, ParamBinding, Tier};

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
    /// An array value; the harness binds it as the target's [`ParamBinding`] requires.
    IntArray(Vec<i64>),
    TextArray(Vec<&'static str>),
    /// JSON text (what an array becomes on SQLite and MySQL).
    Json(String),
}

pub struct Engines {
    pub pg: Client,
    pub lite: rusqlite::Connection,
    /// Present only for the PostgreSQL -> MySQL/MariaDB suites.
    pub mysql: Option<mysql::Conn>,
}

pub fn pg_url() -> String {
    std::env::var("UNISQL_PG_URL")
        .unwrap_or_else(|_| "host=127.0.0.1 port=55432 user=postgres password=unisql".to_string())
}

impl Engines {
    /// Connect to PostgreSQL (must be reachable; this fails loudly otherwise) and open an in-memory
    /// SQLite, then load the same fixture into both.
    pub fn open(pg_fixture: &str, lite_fixture: &str) -> Result<Engines, String> {
        Engines::open_with(pg_fixture, lite_fixture, true)
    }

    /// As [`Engines::open`], choosing SQLite's `case_sensitive_like` setting. SQLite's default is OFF
    /// (case-insensitive `LIKE`); the PostgreSQL->SQLite suite turns it ON because PostgreSQL's
    /// `LIKE` is case-sensitive, and the SQLite->PostgreSQL suite leaves it at the default.
    pub fn open_with(
        pg_fixture: &str,
        lite_fixture: &str,
        case_sensitive_like: bool,
    ) -> Result<Engines, String> {
        let mut pg = Client::connect(&pg_url(), NoTls)
            .map_err(|e| format!("cannot reach PostgreSQL at `{}`: {e}", pg_url()))?;
        // One schema per test (named after its thread), so suites can run in parallel.
        let schema: String = format!(
            "uni_{}",
            std::thread::current()
                .name()
                .unwrap_or("main")
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                })
                .collect::<String>()
        );
        let schema: String = schema.chars().take(60).collect();
        pg.batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path = {schema};"
        ))
        .map_err(|e| format!("pg schema: {e}"))?;
        pg.batch_execute(pg_fixture)
            .map_err(|e| format!("pg fixture: {e}"))?;
        let lite = rusqlite::Connection::open_in_memory().map_err(|e| e.to_string())?;
        // LIKE case sensitivity is a connection setting on SQLite (allowance
        // `like_case_sensitivity_is_connection_setting`); PostgreSQL's LIKE is case-sensitive.
        if case_sensitive_like {
            lite.execute_batch("PRAGMA case_sensitive_like = ON;")
                .map_err(|e| e.to_string())?;
        }
        lite.execute_batch(lite_fixture)
            .map_err(|e| format!("sqlite fixture: {e}"))?;
        Ok(Engines {
            pg,
            lite,
            mysql: None,
        })
    }

    /// PostgreSQL + a MySQL/MariaDB at `UNISQL_MYSQL_URL` (or `UNISQL_MARIADB_URL` when `maria`).
    /// The MySQL database is recreated and loaded with `mysql_fixture`.
    pub fn open_mysql(
        pg_fixture: &str,
        mysql_fixture: &str,
        maria: bool,
    ) -> Result<Engines, String> {
        Engines::open_mysql_with(pg_fixture, "", mysql_fixture, maria, true)
    }

    /// As [`Engines::open_mysql`] with a SQLite fixture too, for suites that read MySQL and write
    /// either PostgreSQL or SQLite. `case_sensitive_like` is SQLite's setting for that connection.
    pub fn open_mysql_with(
        pg_fixture: &str,
        lite_fixture: &str,
        mysql_fixture: &str,
        maria: bool,
        case_sensitive_like: bool,
    ) -> Result<Engines, String> {
        let mut e = Engines::open_with(pg_fixture, lite_fixture, case_sensitive_like)?;
        let (var, default) = if maria {
            (
                "UNISQL_MARIADB_URL",
                "mysql://root:unisql@127.0.0.1:53307/uni",
            )
        } else {
            (
                "UNISQL_MYSQL_URL",
                "mysql://root:unisql@127.0.0.1:53306/uni",
            )
        };
        let url = std::env::var(var).unwrap_or_else(|_| default.to_string());
        let opts = mysql::Opts::from_url(&url).map_err(|e| format!("bad {var}: {e}"))?;
        let mut conn = mysql::Conn::new(opts)
            .map_err(|e| format!("cannot reach MySQL/MariaDB at `{url}`: {e}"))?;
        use mysql::prelude::Queryable;
        // One database per test (named after its thread), so suites can run in parallel.
        let db: String = format!(
            "uni_{}",
            std::thread::current()
                .name()
                .unwrap_or("main")
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                })
                .collect::<String>()
        )
        .chars()
        .take(60)
        .collect();
        conn.query_drop(format!("DROP DATABASE IF EXISTS {db}"))
            .map_err(|e| e.to_string())?;
        conn.query_drop(format!("CREATE DATABASE {db}"))
            .map_err(|e| e.to_string())?;
        conn.query_drop(format!("USE {db}"))
            .map_err(|e| e.to_string())?;
        for stmt in mysql_fixture
            .split(";\n")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            conn.query_drop(stmt)
                .map_err(|e| format!("mysql fixture: {e}\n  in: {stmt}"))?;
        }
        e.mysql = Some(conn);
        Ok(e)
    }

    pub fn mysql_version(&mut self) -> String {
        use mysql::prelude::Queryable;
        self.mysql
            .as_mut()
            .and_then(|c| {
                c.query_first::<String, _>("SELECT VERSION()")
                    .ok()
                    .flatten()
            })
            .unwrap_or_default()
    }

    pub fn run_mysql(
        &mut self,
        sql: &str,
        params: &[Param],
        after: Option<&str>,
    ) -> Result<(Rows, Rows), String> {
        use mysql::prelude::Queryable;
        let conn = self.mysql.as_mut().ok_or("no MySQL connection")?;
        let binds: Vec<mysql::Value> = params
            .iter()
            .map(|p| match p {
                Param::Null => mysql::Value::NULL,
                Param::Int(i) => mysql::Value::Int(*i),
                Param::Real(r) => mysql::Value::Double(*r),
                Param::Json(j) => mysql::Value::Bytes(j.as_bytes().to_vec()),
                Param::IntArray(_) | Param::TextArray(_) => mysql::Value::NULL,
                Param::Text(t) => mysql::Value::Bytes(t.as_bytes().to_vec()),
            })
            .collect();
        let mut tx = conn
            .start_transaction(mysql::TxOpts::default())
            .map_err(|e| e.to_string())?;
        let out = (|| {
            let stmt = tx.prep(sql).map_err(|e| format!("prepare: {e}"))?;
            let rs: Vec<mysql::Row> = tx
                .exec(&stmt, binds.clone())
                .map_err(|e| format!("execute: {e}"))?;
            let rows = rs.iter().map(mysql_row).collect::<Result<Rows, String>>()?;
            let post = match after {
                None => Vec::new(),
                Some(q) => {
                    // Binary protocol (prepared), so integers arrive as integers, not text.
                    let st = tx.prep(q).map_err(|e| format!("after: {e}"))?;
                    let rs: Vec<mysql::Row> =
                        tx.exec(&st, ()).map_err(|e| format!("after: {e}"))?;
                    rs.iter().map(mysql_row).collect::<Result<Rows, String>>()?
                }
            };
            Ok((rows, post))
        })();
        let _ = tx.rollback();
        out
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
    pub fn run_pg(
        &mut self,
        sql: &str,
        params: &[Param],
        after: Option<&str>,
    ) -> Result<(Rows, Rows), String> {
        let mut tx = self.pg.transaction().map_err(|e| e.to_string())?;
        let out = (|| {
            let stmt = tx.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
            let owned = pg_params(stmt.params(), params)?;
            let refs: Vec<&(dyn ToSql + Sync)> = owned.iter().map(|b| b.as_ref()).collect();
            let rows = if stmt.columns().is_empty() {
                tx.execute(&stmt, &refs)
                    .map_err(|e| format!("execute: {e}"))?;
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

    pub fn run_sqlite(
        &mut self,
        sql: &str,
        params: &[Param],
        after: Option<&str>,
    ) -> Result<(Rows, Rows), String> {
        let tx = self.lite.transaction().map_err(|e| e.to_string())?;
        let out = (|| {
            let mut stmt = tx.prepare(sql).map_err(|e| format!("prepare: {e}"))?;
            let binds: Vec<rusqlite::types::Value> = params
                .iter()
                .map(|p| match p {
                    Param::Null => rusqlite::types::Value::Null,
                    Param::Int(i) => rusqlite::types::Value::Integer(*i),
                    Param::Real(r) => rusqlite::types::Value::Real(*r),
                    Param::Json(j) => rusqlite::types::Value::Text(j.clone()),
                    Param::IntArray(_) | Param::TextArray(_) => rusqlite::types::Value::Null,
                    Param::Text(t) => rusqlite::types::Value::Text((*t).to_string()),
                })
                .collect();
            let ncols = stmt.column_count();
            let rows = if ncols == 0 {
                stmt.execute(rusqlite::params_from_iter(binds.iter()))
                    .map_err(|e| format!("execute: {e}"))?;
                Vec::new()
            } else {
                let mut out = Vec::new();
                let mut q = stmt
                    .query(rusqlite::params_from_iter(binds.iter()))
                    .map_err(|e| format!("query: {e}"))?;
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
        return Err(format!(
            "statement has {} parameters, case supplies {}",
            types.len(),
            params.len()
        ));
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
            (Type::INT4_ARRAY, Param::IntArray(v)) => {
                Box::new(v.iter().map(|x| *x as i32).collect::<Vec<i32>>())
            }
            (Type::INT8_ARRAY, Param::IntArray(v)) => Box::new(v.clone()),
            (Type::TEXT_ARRAY | Type::VARCHAR_ARRAY, Param::TextArray(v)) => {
                Box::new(v.iter().map(|s| s.to_string()).collect::<Vec<String>>())
            }
            (Type::INT4_ARRAY, Param::Null) => Box::new(None::<Vec<i32>>),
            (Type::INT8_ARRAY, Param::Null) => Box::new(None::<Vec<i64>>),
            (Type::TEXT_ARRAY | Type::VARCHAR_ARRAY, Param::Null) => Box::new(None::<Vec<String>>),
            (Type::TEXT | Type::VARCHAR | Type::UNKNOWN, Param::Text(s)) => {
                Box::new((*s).to_string())
            }
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
            Type::TEXT | Type::VARCHAR | Type::NAME | Type::BPCHAR => {
                r.get::<_, Option<String>>(i).map(Cell::Text)
            }
            ref other => {
                return Err(format!(
                    "harness cannot read PostgreSQL type {other} (cast it in the statement)"
                ));
            }
        };
        out.push(cell.unwrap_or(Cell::Null));
    }
    Ok(out)
}

fn mysql_row(r: &mysql::Row) -> Result<Vec<Cell>, String> {
    use mysql::consts::ColumnType as T;
    let mut out = Vec::new();
    for (i, col) in r.columns_ref().iter().enumerate() {
        let v: mysql::Value = r.as_ref(i).cloned().ok_or("missing column")?;
        out.push(match v {
            mysql::Value::NULL => Cell::Null,
            mysql::Value::Int(x) => Cell::Int(x),
            mysql::Value::UInt(x) => Cell::Int(x as i64),
            mysql::Value::Float(x) => Cell::Real(x as f64),
            mysql::Value::Double(x) => Cell::Real(x),
            mysql::Value::Bytes(b) => {
                let t = String::from_utf8_lossy(&b).into_owned();
                match col.column_type() {
                    // DECIMAL arrives as text: SUM(int) is `850`, AVG is `283.3333`. This is the
                    // engine's value codec, the same kind of per-engine mapping as booleans.
                    T::MYSQL_TYPE_NEWDECIMAL | T::MYSQL_TYPE_DECIMAL => {
                        if t.contains('.') {
                            Cell::Real(
                                t.parse::<f64>()
                                    .map_err(|e| format!("decimal `{t}`: {e}"))?,
                            )
                        } else {
                            Cell::Int(
                                t.parse::<i64>()
                                    .map_err(|e| format!("decimal `{t}`: {e}"))?,
                            )
                        }
                    }
                    _ => Cell::Text(t),
                }
            }
            other => Cell::Text(format!("{other:?}")),
        });
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
    a.iter()
        .zip(&b)
        .all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| cells_equal(p, q)))
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
    /// `(construct, allowance)` for every Conditional entry the report recorded.
    pub allowances: Vec<(String, Allowance)>,
    /// `(construct, reason)` when the case passed by being refused.
    pub refusal: Option<(String, String)>,
    pub rows: usize,
}

/// Which way a suite translates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    PgToSqlite,
    SqliteToPg,
    PgToMysql,
    PgToMariadb,
    MysqlToPg,
    MysqlToSqlite,
    MariadbToPg,
}

impl Dir {
    pub fn source(self) -> &'static str {
        match self {
            Dir::PgToSqlite | Dir::PgToMysql | Dir::PgToMariadb => "postgres",
            Dir::SqliteToPg => "sqlite",
            Dir::MysqlToPg | Dir::MysqlToSqlite => "mysql",
            Dir::MariadbToPg => "mariadb",
        }
    }
    pub fn target(self) -> &'static str {
        match self {
            Dir::PgToSqlite => "sqlite",
            Dir::SqliteToPg => "postgres",
            Dir::PgToMysql => "mysql",
            Dir::PgToMariadb => "mariadb",
            Dir::MysqlToPg | Dir::MariadbToPg => "postgres",
            Dir::MysqlToSqlite => "sqlite",
        }
    }
}

impl Engines {
    fn run_on(
        &mut self,
        engine: &str,
        sql: &str,
        params: &[Param],
        after: Option<&str>,
    ) -> Result<(Rows, Rows), String> {
        match engine {
            "postgres" => self.run_pg(sql, params, after),
            "sqlite" => self.run_sqlite(sql, params, after),
            "mysql" | "mariadb" => self.run_mysql(sql, params, after),
            other => Err(format!("no engine `{other}`")),
        }
    }
}

/// Bind values for the target: numbered-placeholder targets use the caller's order; positional `?`
/// targets (MySQL) bind in `param_order`, repeating a value used twice.
fn target_params(
    target: &str,
    params: &[Param],
    order: &[u32],
    bindings: &[ParamBinding],
) -> Vec<Param> {
    let conv = |n: usize| -> Param {
        let p = &params[n];
        match (p, bindings.get(n).copied().unwrap_or(ParamBinding::Scalar)) {
            (Param::IntArray(v), ParamBinding::JsonArrayText) => Param::Json(format!(
                "[{}]",
                v.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            (Param::TextArray(v), ParamBinding::JsonArrayText) => Param::Json(format!(
                "[{}]",
                v.iter()
                    .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            _ => p.clone(),
        }
    };
    if matches!(target, "mysql" | "mariadb") {
        order.iter().map(|n| conv((*n - 1) as usize)).collect()
    } else {
        (0..params.len()).map(conv).collect()
    }
}

pub fn run_case(e: &mut Engines, c: &Case) -> Outcome {
    run_case_dir(e, c, Dir::PgToSqlite)
}

pub fn run_case_dir(e: &mut Engines, c: &Case, dir: Dir) -> Outcome {
    let (src, tgt) = (dir.source(), dir.target());
    let opts = c.allow.iter().fold(Options::default(), |o, a| o.with(*a));
    let mut out = Outcome {
        name: c.name,
        passed: false,
        detail: String::new(),
        translated: None,
        constructs: vec![],
        allowances: vec![],
        refusal: None,
        rows: 0,
    };
    let tr = unisql::translate(c.sql, src, tgt, &opts);
    match (&c.expect, tr) {
        (Expect::Refused(construct), Err(unisql::Error::Untranslatable(u))) => {
            out.passed = u.construct == *construct || u.construct.starts_with(construct);
            out.refusal = Some((u.construct.clone(), u.reason.clone()));
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
            match e.run_on(src, c.sql, &c.params, None) {
                Ok(_) => {
                    out.passed = true;
                    out.detail = format!("parser gap (the engine accepts it): {msg}");
                }
                Err(pe) => {
                    out.detail =
                        format!("the engine also rejects it, so this is not a parser gap: {pe}")
                }
            }
        }
        (
            Expect::NotInIr(construct),
            Err(unisql::Error::Read(unisql::ReadError::NotInIr {
                construct: got,
                detail,
            })),
        ) => {
            out.passed = got == *construct;
            out.detail = format!("not in IR `{got}`: {detail}");
        }
        (Expect::Diverges, Ok(t)) => {
            out.translated = Some(t.sql.clone());
            let tparams = target_params(tgt, &c.params, &t.param_order, &t.param_bindings);
            let orig = e.run_on(src, c.sql, &c.params, c.after);
            let other = e.run_on(tgt, &t.sql, &tparams, c.after);
            match (orig, other) {
                (Err(pe), Ok(l)) => {
                    out.passed = true;
                    out.detail = format!("diverges: {src} errors ({pe}); {tgt} returns {:?}", l.0);
                }
                (Ok(o), Err(le)) => {
                    out.passed = true;
                    out.detail = format!("diverges: {src} returns {:?}; {tgt} errors ({le})", o.0);
                }
                (Ok(o), Ok(l))
                    if !rows_equal(&o.0, &l.0, c.ordered) || !rows_equal(&o.1, &l.1, c.ordered) =>
                {
                    out.passed = true;
                    out.detail = format!("diverges: {src} {:?} vs {tgt} {:?}", o.0, l.0);
                }
                (o, l) => {
                    out.detail = format!(
                        "expected a divergence but engines agree/failed alike: {:?} / {:?}",
                        o.map(|x| x.0),
                        l.map(|x| x.0)
                    )
                }
            }
        }
        (
            Expect::Refused(_) | Expect::NeedsAllowance(_) | Expect::NotInIr(_) | Expect::ParserGap,
            Ok(t),
        ) => {
            out.detail = format!("expected a refusal but got text: {}", t.sql);
            out.translated = Some(t.sql);
        }
        (_, Err(err)) => out.detail = format!("translate failed: {err}"),
        (Expect::Same, Ok(t)) => {
            out.translated = Some(t.sql.clone());
            out.constructs = t
                .report
                .entries
                .iter()
                .map(|x| (x.construct.clone(), x.tier))
                .collect();
            out.allowances = t
                .report
                .entries
                .iter()
                .filter_map(|x| x.allowance.map(|a| (x.construct.clone(), a)))
                .collect();
            // Identity check: source -> source must give the same results as the original.
            let self_t = unisql::translate(c.sql, src, src, &Options::default());
            let self_text = match self_t {
                Ok(p) => p.sql,
                Err(err) => {
                    out.detail = format!("{src}->{src} failed: {err}");
                    return out;
                }
            };
            let tparams = target_params(tgt, &c.params, &t.param_order, &t.param_bindings);
            let orig = e.run_on(src, c.sql, &c.params, c.after);
            let ident = e.run_on(src, &self_text, &c.params, c.after);
            let other = e.run_on(tgt, &t.sql, &tparams, c.after);
            match (orig, ident, other) {
                (Ok(o), Ok(i), Ok(l)) => {
                    out.rows = o.0.len();
                    if !rows_equal(&o.0, &i.0, c.ordered) || !rows_equal(&o.1, &i.1, c.ordered) {
                        out.detail =
                            format!("{src}->{src} differs from original; text: {self_text}");
                    } else if !rows_equal(&o.0, &l.0, c.ordered) {
                        out.detail = format!(
                            "RESULT DIFFERS
  {src}: {:?}
  {tgt}: {:?}
  sql: {}",
                            o.0, l.0, t.sql
                        );
                    } else if !rows_equal(&o.1, &l.1, c.ordered) {
                        out.detail = format!(
                            "STATE DIFFERS
  {src}: {:?}
  {tgt}: {:?}
  sql: {}",
                            o.1, l.1, t.sql
                        );
                    } else {
                        out.passed = true;
                        out.detail = "same".into();
                    }
                }
                (o, i, l) => {
                    out.detail = format!(
                        "engine error: {src}={:?} {src}->{src}={:?} {tgt}={:?}
  sql: {}",
                        o.err(),
                        i.err(),
                        l.err(),
                        t.sql
                    )
                }
            }
        }
    }
    out
}

/// The fixture constructs a matrix: distinct construct ids across passing cases.
pub fn matrix_rows(outcomes: &[Outcome]) -> BTreeSet<(String, &'static str)> {
    let mut m = BTreeSet::new();
    for o in outcomes
        .iter()
        .filter(|o| o.passed && o.translated.is_some())
    {
        for (c, t) in &o.constructs {
            m.insert((c.clone(), t.name()));
        }
    }
    m
}

/// Matrix rows in PersistAnt's agreed schema (agreed with the PersistAnt lane 2026-10-12):
/// `subject_kind, subject, backend, version, how, evidence, note, from`.
///
/// - `how`: `native` (Exact), `emulated` (Emulated), `conditional` (Conditional: the construct
///   translates only if the call site names the allowance, listed as `allowance=a,b` at the start of
///   `note`, so a gate can read it), `absent` (refused). Weakest proven form wins per subject.
/// - `evidence`: `proven` only for a construct that translated and whose result matched on the real
///   engines; a refusal is `declared` (the translator refuses; the engine's lack is not itself tested).
/// - A refusal is always its own subject `<construct>.refused` (matching
///   `Untranslatable::subject()`), never a note on a proven cell, so a gate that keys on subject
///   sees the split.
pub fn matrix_tsv(outcomes: &[Outcome], backend: &str, version: &str, from: &str) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let rank = |h: &str| match h {
        "native" => 0,
        "emulated" => 1,
        "conditional" => 2,
        _ => 3,
    };
    let mut proven: BTreeMap<String, (String, BTreeSet<&'static str>)> = BTreeMap::new();
    for o in outcomes.iter().filter(|o| o.passed) {
        for (c, t) in &o.constructs {
            let how = match t {
                Tier::Exact => "native",
                Tier::Emulated => "emulated",
                _ => "conditional",
            };
            let e = proven
                .entry(c.clone())
                .or_insert((how.to_string(), BTreeSet::new()));
            if rank(how) > rank(&e.0) {
                e.0 = how.to_string();
            }
        }
        for (c, a) in &o.allowances {
            if let Some(e) = proven.get_mut(c) {
                e.1.insert(a.name());
            }
        }
    }
    let mut refused: BTreeMap<String, String> = BTreeMap::new();
    for o in outcomes.iter().filter(|o| o.passed) {
        if let Some((c, why)) = &o.refusal {
            refused
                .entry(c.clone())
                .or_insert_with(|| why.replace(['\t', '\n'], " "));
        }
    }
    let mut out =
        String::from("subject_kind\tsubject\tbackend\tversion\thow\tevidence\tnote\tfrom\n");
    let mut row = |subject: &str, how: &str, ev: &str, note: &str| {
        out.push_str(&format!(
            "construct\t{subject}\t{backend}\t{version}\t{how}\t{ev}\t{note}\t{from}\n"
        ));
    };
    let mut subjects: BTreeSet<String> = proven.keys().cloned().collect();
    subjects.extend(refused.keys().cloned());
    for s in subjects {
        match (proven.get(&s), refused.get(&s)) {
            (Some((how, al)), r) => {
                let note = if al.is_empty() {
                    String::new()
                } else {
                    format!(
                        "allowance={}",
                        al.iter().copied().collect::<Vec<_>>().join(",")
                    )
                };
                row(&s, how, "proven", &note);
                if let Some(why) = r {
                    row(&format!("{s}.refused"), "absent", "declared", why);
                }
            }
            (None, Some(why)) => row(&format!("{s}.refused"), "absent", "declared", why),
            (None, None) => {}
        }
    }
    out
}
