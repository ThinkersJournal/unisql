//! Throwaway measurement tool (design phase). Reads JSON lines produced by `extract.py` on stdin,
//! parses each SQL text as PostgreSQL with `sqlparser`, and tags every construct with a proposed
//! tier for a SQLite target. It emits no SQL. Tiers are a *rule-table proposal*, not a result of
//! running the statements (DESIGN.md section 11).
//!
//! Output: one JSON line per input line, plus a final summary is computed by `summarise.py`.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::ops::ControlFlow;

use sqlparser::ast::{visit_expressions, visit_statements, Expr, Statement};
use sqlparser::dialect::{PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Tier {
    Exact,
    Emulated,
    Conditional,
    Refused,
}

impl Tier {
    fn name(self) -> &'static str {
        match self {
            Tier::Exact => "exact",
            Tier::Emulated => "emulated",
            Tier::Conditional => "conditional",
            Tier::Refused => "refused",
        }
    }
}

/// Functions with the same name and meaning on SQLite (>= 3.35 assumed; UNVERIFIED which minimum).
const EXACT_FNS: &[&str] = &[
    "count", "sum", "min", "max", "avg", "coalesce", "nullif", "lower", "upper", "length", "abs",
    "round", "trim", "ltrim", "rtrim", "replace", "substr", "substring", "row_number", "rank",
    "dense_rank", "lag", "lead", "first_value", "last_value", "ifnull", "instr", "hex",
    "greatest", "least", "concat",
];

/// Same intent, different spelling or a stated difference on SQLite: Emulated.
const EMULATED_FNS: &[(&str, &str)] = &[
    ("string_agg", "group_concat"),
    ("now", "CURRENT_TIMESTAMP (text, not timestamptz)"),
    ("current_timestamp", "text, not timestamptz"),
    ("gen_random_uuid", "randomblob-based expression"),
    ("date_trunc", "strftime composition"),
    ("to_char", "strftime"),
    ("char_length", "length"),
    ("btrim", "trim"),
    ("left", "substr"),
    ("right", "substr"),
    ("position", "instr"),
    ("starts_with", "like/substr"),
];

fn variant(e: &impl std::fmt::Debug) -> String {
    let s = format!("{e:?}");
    s.split(|c: char| c == '(' || c == ' ' || c == '{' || c == '[')
        .next()
        .unwrap_or("")
        .to_string()
}

fn classify_expr(e: &Expr, out: &mut BTreeSet<(Tier, String)>) {
    match e {
        Expr::Function(f) => {
            let name = f.name.to_string().to_lowercase();
            let base = name.rsplit('.').next().unwrap_or(&name).to_string();
            if base.starts_with("pg_advisory") || base == "pg_try_advisory_lock" {
                out.insert((Tier::Refused, format!("fn:{base} (advisory lock)")));
            } else if matches!(base.as_str(), "set_config" | "current_setting") {
                out.insert((Tier::Refused, format!("fn:{base} (session setting)")));
            } else if base.starts_with("pg_") || base.starts_with("jsonb_") || base.starts_with("json_") {
                out.insert((Tier::Refused, format!("fn:{base} (engine/JSON function)")));
            } else if matches!(base.as_str(), "array_agg" | "unnest" | "array_length" | "cardinality") {
                out.insert((Tier::Refused, format!("fn:{base} (array)")));
            } else if let Some((_, why)) = EMULATED_FNS.iter().find(|(n, _)| *n == base) {
                // time functions change the value's type, so they are Conditional, not Emulated.
                let t = if matches!(base.as_str(), "now" | "current_timestamp" | "date_trunc" | "to_char") {
                    Tier::Conditional
                } else {
                    Tier::Emulated
                };
                out.insert((t, format!("fn:{base} -> {why}")));
            } else if EXACT_FNS.contains(&base.as_str()) {
                // exact: nothing recorded beyond presence
            } else {
                out.insert((Tier::Refused, format!("fn:{base} (no mapping)")));
            }
        }
        Expr::ILike { .. } => {
            out.insert((Tier::Conditional, "ILIKE (collation/case rule)".into()));
        }
        Expr::Cast { .. } => {
            out.insert((Tier::Emulated, "cast (type names differ)".into()));
        }
        Expr::Array(_) => {
            out.insert((Tier::Refused, "ARRAY[..] literal".into()));
        }
        Expr::AnyOp { .. } | Expr::AllOp { .. } => {
            out.insert((Tier::Refused, "= ANY/ALL (array operand)".into()));
        }
        Expr::Interval(_) => {
            out.insert((Tier::Conditional, "INTERVAL arithmetic".into()));
        }
        Expr::BinaryOp { op, .. } => {
            let v = variant(op);
            match v.as_str() {
                "Arrow" | "LongArrow" | "HashArrow" | "HashLongArrow" | "AtArrow" | "ArrowAt"
                | "Question" | "QuestionAnd" | "QuestionPipe" | "AtAt" | "HashMinus" => {
                    out.insert((Tier::Refused, format!("op:{v} (JSON/array/search operator)")));
                }
                "StringConcat" => {
                    out.insert((Tier::Conditional, "|| (NULL propagation differs)".into()));
                }
                "Divide" => {
                    out.insert((Tier::Conditional, "/ (integer division rule)".into()));
                }
                _ => {}
            }
        }
        Expr::Extract { .. } => {
            out.insert((Tier::Emulated, "EXTRACT -> strftime".into()));
        }
        _ => {}
    }
}

fn classify_stmt(s: &Statement, text: &str, out: &mut BTreeSet<(Tier, String)>) {
    let v = variant(s);
    match v.as_str() {
        "Query" | "Insert" | "Update" | "Delete" | "Merge" => {}
        "StartTransaction" | "Commit" | "Rollback" | "Savepoint" | "ReleaseSavepoint" => {
            let d = format!("{s:?}");
            if d.contains("IsolationLevel") || d.contains("ReadOnly") {
                out.insert((Tier::Refused, "transaction isolation/access mode".into()));
            }
        }
        "CreateTable" | "CreateIndex" | "AlterTable" | "Drop" | "CreateView" | "CreateSchema" => {
            out.insert((Tier::Emulated, format!("DDL:{v} (per-engine migration, section 3.3)")));
        }
        "CreateTrigger" | "DropTrigger" => {
            out.insert((Tier::Refused, "trigger".into()));
        }
        "CreateFunction" | "DropFunction" | "CreateProcedure" | "DropProcedure" | "Call" => {
            out.insert((Tier::Refused, "function/procedure".into()));
        }
        "Copy" | "CopyIntoSnowflake" => {
            out.insert((Tier::Refused, "COPY".into()));
        }
        "CreateExtension" | "DropExtension" => {
            out.insert((Tier::Refused, "extension".into()));
        }
        "Set" | "SetVariable" => {
            out.insert((Tier::Refused, "SET session variable".into()));
        }
        "ShowVariable" | "ShowVariables" => {
            out.insert((Tier::Refused, "SHOW".into()));
        }
        "Listen" | "Notify" | "Unlisten" => {
            out.insert((Tier::Refused, "LISTEN/NOTIFY".into()));
        }
        "Truncate" => {
            out.insert((Tier::Emulated, "TRUNCATE -> DELETE".into()));
        }
        other => {
            out.insert((Tier::Refused, format!("stmt:{other} (no mapping)")));
        }
    }
    // Query-level markers checked on the Debug rendering; validated by controls in --controls.
    let d = format!("{s:?}");
    if d.contains("LockClause") || d.contains("locks: [") && !d.contains("locks: []") {
        out.insert((Tier::Refused, "row lock (FOR UPDATE/SHARE)".into()));
    }
    if d.contains("SkipLocked") {
        out.insert((Tier::Refused, "SKIP LOCKED".into()));
    }
    if d.contains("distinct: Some(On(") {
        out.insert((Tier::Emulated, "DISTINCT ON -> window function".into()));
    }
    if d.contains("OnConflict(") || d.contains("on: Some(OnConflict") {
        let _ = text;
        out.insert((Tier::Emulated, "ON CONFLICT upsert (WHERE form UNVERIFIED on SQLite)".into()));
    }
    if d.contains("returning: Some(") {
        out.insert((Tier::Emulated, "RETURNING (needs SQLite >= 3.35)".into()));
    }
    if d.contains("Placeholder(\"$") {
        // positional placeholder rewrite is mechanical; recorded so counts show it
        out.insert((Tier::Exact, "placeholder $n -> ?n".into()));
    }
}

/// Replace Rust `format!` holes with an identifier so a templated statement can still be parsed.
fn de_template(sql: &str) -> (String, bool) {
    let mut out = String::with_capacity(sql.len());
    let b: Vec<char> = sql.chars().collect();
    let (mut i, mut n, mut hit) = (0, 0, false);
    while i < b.len() {
        if b[i] == '{' && i + 1 < b.len() && b[i + 1] != '{' {
            if let Some(j) = b[i..].iter().position(|&c| c == '}') {
                let inner: String = b[i + 1..i + j].iter().collect();
                if inner.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '?' || c == '.') {
                    n += 1;
                    hit = true;
                    out.push_str(&format!("tpl_{n}"));
                    i += j + 1;
                    continue;
                }
            }
        }
        if b[i] == '{' && i + 1 < b.len() && b[i + 1] == '{' {
            out.push('{');
            i += 2;
            continue;
        }
        if b[i] == '}' && i + 1 < b.len() && b[i + 1] == '}' {
            out.push('}');
            i += 2;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    (out, hit)
}

/// Remove `DO $tag$ ... $tag$;` blocks (sqlparser cannot parse DO). Returns (rest, blocks_removed).
fn strip_do_blocks(sql: &str) -> (String, usize) {
    let lower = sql.to_lowercase();
    let mut out = String::new();
    let (mut pos, mut n) = (0usize, 0usize);
    while let Some(rel) = lower[pos..].find("do $") {
        let at = pos + rel;
        let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric() && lower.as_bytes()[at - 1] != b'_';
        let tag_start = at + 3;
        let Some(tag_len) = lower[tag_start + 1..].find('$') else { break };
        let tag = &sql[tag_start..tag_start + tag_len + 2];
        let body_from = tag_start + tag.len();
        match (before_ok, sql[body_from..].find(tag)) {
            (true, Some(close)) => {
                out.push_str(&sql[pos..at]);
                pos = body_from + close + tag.len();
                if sql[pos..].trim_start().starts_with(';') {
                    pos += sql[pos..].find(';').unwrap() + 1;
                }
                n += 1;
            }
            _ => {
                out.push_str(&sql[pos..tag_start]);
                pos = tag_start;
            }
        }
    }
    out.push_str(&sql[pos..]);
    (out, n)
}

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.expect("stdin");
        let mut rec: serde_json::Value = serde_json::from_str(&line).expect("json line");
        let sql = rec["sql"].as_str().unwrap_or("").to_string();
        let (sql_nodo, do_blocks) = strip_do_blocks(&sql);
        let file = rec["file"].as_str().unwrap_or("").to_lowercase();
        let sqlite_src = file.contains("sqlite");
        let mysql_src = file.contains("mysql");
        let parse = |t: &str| {
            if mysql_src {
                Parser::parse_sql(&sqlparser::dialect::MySqlDialect {}, t)
            } else if sqlite_src {
                Parser::parse_sql(&SQLiteDialect {}, t)
            } else {
                Parser::parse_sql(&PostgreSqlDialect {}, t)
            }
        };
        // Parse as written first; only a failing text is de-templated (a `{x}` inside a string
        // literal or comment is not a format! hole).
        let (sql2, templated) = if parse(&sql_nodo).is_ok() {
            (sql_nodo.clone(), false)
        } else {
            de_template(&sql_nodo)
        };
        rec["source_dialect"] = (if sqlite_src { "sqlite" } else if mysql_src { "mysql" } else { "postgres" }).into();
        rec["do_blocks"] = do_blocks.into();
        let parsed = parse(&sql2);
        let lower = sql2.trim_start().to_lowercase();
        let is_do_block = lower.starts_with("do ") && (lower.contains("$$") || lower.contains("begin"));
        match parsed {
            Err(_) if is_do_block => {
                rec["parsed"] = true.into();
                rec["statements"] = 1.into();
                rec["kinds"] = vec!["DoBlock"].into();
                rec["tier"] = "refused".into();
                rec["reasons"] = vec!["refused:DO block (procedural; sqlparser cannot parse it)"].into();
            }
            Err(e) => {
                rec["parsed"] = false.into();
                rec["error"] = e.to_string().into();
            }
            Ok(stmts) => {
                let mut set: BTreeSet<(Tier, String)> = BTreeSet::new();
                for s in &stmts {
                    classify_stmt(s, &sql2, &mut set);
                }
                let _ = visit_expressions(&stmts, |e| {
                    classify_expr(e, &mut set);
                    ControlFlow::<()>::Continue(())
                });
                let mut kinds = Vec::new();
                let _ = visit_statements(&stmts, |s| {
                    kinds.push(variant(s));
                    ControlFlow::<()>::Continue(())
                });
                if do_blocks > 0 {
                    set.insert((Tier::Refused, format!("DO block x{do_blocks} (procedural)")));
                }
                let worst = set.iter().map(|(t, _)| *t).max().unwrap_or(Tier::Exact);
                rec["parsed"] = true.into();
                rec["statements"] = stmts.len().into();
                rec["kinds"] = kinds.into();
                rec["tier"] = worst.name().into();
                rec["reasons"] = set
                    .iter()
                    .filter(|(t, _)| *t != Tier::Exact)
                    .map(|(t, r)| format!("{}:{}", t.name(), r))
                    .collect::<Vec<_>>()
                    .into();
            }
        }
        rec["templated"] = templated.into();
        rec.as_object_mut().unwrap().remove("sql_full");
        writeln!(out, "{}", rec).expect("stdout");
    }
}
