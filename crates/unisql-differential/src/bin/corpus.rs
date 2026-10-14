// SPDX-License-Identifier: MIT OR Apache-2.0
//! Corpus runner: reads JSON lines `{id, group, sql}` on stdin, runs the real translator
//! PostgreSQL -> SQLite on each, and prints one JSON line per statement. No engine is contacted.
//!
//! status: `translated` (no allowance needed), `conditional` (translates only with the listed
//! allowances), `refused` (construct), `not_in_ir` (construct), `syntax` (parser rejected the text).

use std::io::BufRead;

use unisql::{Allowance, Error, Options, Tier};

const ALL: &[Allowance] = &[
    Allowance::NowAsText,
    Allowance::TimestampAsText,
    Allowance::EpochWholeSeconds,
    Allowance::TimestampWholeSeconds,
    Allowance::PipesAsLogicalOr,
    Allowance::IgnoreCoversOnlyUniqueConflicts,
    Allowance::SubstringStartAssumedPositive,
    Allowance::DateAsText,
    Allowance::LikeCaseSensitivityIsConnectionSetting,
    Allowance::AsciiOnlyCaseFolding,
    Allowance::LenientCast,
    Allowance::DivisionByZeroIsNull,
    Allowance::TextCollation,
    Allowance::BooleanAsInteger,
    Allowance::TimestampArithmeticAsText,
    Allowance::JsonScalarAffinity,
    Allowance::JsonRepresentationAsText,
    Allowance::IntegerDivisionAssumed,
    Allowance::UpsertAnyUniqueKey,
    Allowance::AggregateStringTruncation,
    Allowance::ArrayNullSemantics,
];

fn main() {
    let mut all = ALL.iter().fold(Options::default(), |o, a| o.with(*a));
    // Optional second argument: a DDL script (migrations) whose column types become a `Schema`.
    if let Some(path) = std::env::args().nth(2) {
        let ddl = std::fs::read_to_string(&path).expect("DDL file");
        let d = unisql::schema_from_ddl(&ddl, "postgres").expect("DDL reads");
        eprintln!("schema from {path}: {} statements skipped", d.skipped.len());
        all = all.with_schema(d.schema);
    }
    // The dialect the statements are written in (default postgres); `UNISQL_SOURCE=sqlite|mysql`.
    let source = std::env::var("UNISQL_SOURCE").unwrap_or_else(|_| "postgres".to_string());
    let target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sqlite".to_string());
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let mut rec: serde_json::Value = serde_json::from_str(&line).unwrap();
        let sql = rec["sql"].as_str().unwrap_or("").to_string();
        // What the reader said about a construct it does not represent (empty otherwise): without
        // it a `not_in_ir  expression` row cannot be told apart from its siblings.
        let mut note = String::new();
        let (status, detail, allowances): (&str, String, Vec<String>) =
            match unisql::translate(&sql, &source, &target, &all) {
                Ok(t) => {
                    let needed: Vec<String> = t
                        .report
                        .entries
                        .iter()
                        .filter(|e| e.tier == Tier::Conditional)
                        .filter_map(|e| e.allowance.map(|a| a.name().to_string()))
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    if needed.is_empty() {
                        ("translated", String::new(), vec![])
                    } else {
                        ("conditional", String::new(), needed)
                    }
                }
                Err(Error::Untranslatable(u)) => ("refused", u.construct, vec![]),
                Err(Error::Read(unisql::ReadError::NotInIr { construct, detail })) => {
                    note = detail.chars().take(120).collect();
                    ("not_in_ir", construct, vec![])
                }
                Err(Error::Read(unisql::ReadError::Syntax(m))) => {
                    ("syntax", m.chars().take(80).collect(), vec![])
                }
                Err(e) => ("error", e.to_string(), vec![]),
            };
        let obj = rec.as_object_mut().unwrap();
        obj.remove("sql");
        obj.insert("status".into(), status.into());
        obj.insert("detail".into(), detail.into());
        obj.insert("note".into(), note.into());
        obj.insert("allowances".into(), allowances.into());
        println!("{rec}");
    }
}
