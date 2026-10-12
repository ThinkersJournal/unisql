//! The writer side of a dialect: a [`Target`] supplies *differences*; the shared printer in
//! [`crate::print`] does the rest. Defaults describe PostgreSQL, the first source dialect, so a
//! Postgres target is nearly empty and every other target overrides only what it must.

use crate::ast::*;
use crate::report::{Allowance, Entry, Options, Report, Tier, Untranslatable};

/// Per-translation state handed to every hook.
pub struct Ctx<'a> {
    pub report: Report,
    pub opts: &'a Options,
    pub source: &'a str,
    pub target: &'a str,
    pub max_param: u32,
    pub param_order: Vec<u32>,
}

impl<'a> Ctx<'a> {
    pub fn new(opts: &'a Options, source: &'a str, target: &'a str) -> Self {
        Ctx { report: Report::default(), opts, source, target, max_param: 0, param_order: Vec::new() }
    }

    /// Record how the target gives `construct`. `Refused` and an *ungranted* `Conditional` return an
    /// error, so the caller `?`s and no SQL text is ever produced for them.
    pub fn note(
        &mut self,
        construct: impl Into<String>,
        tier: Tier,
        note: impl Into<String>,
        allowance: Option<Allowance>,
    ) -> Result<(), Untranslatable> {
        let construct = construct.into();
        let note = note.into();
        match tier {
            Tier::Refused => Err(self.refuse(&construct, &note, &[])),
            Tier::Conditional => {
                let a = allowance.expect("a Conditional note must name its Allowance");
                if !self.opts.allow.contains(&a) {
                    return Err(self.refuse(
                        &construct,
                        &format!("conditional: needs allowance `{}`: {}", a.name(), note),
                        &[],
                    ));
                }
                self.report.push(Entry { construct, tier, note, allowance: Some(a) });
                Ok(())
            }
            _ => {
                self.report.push(Entry { construct, tier, note, allowance });
                Ok(())
            }
        }
    }

    pub fn refuse(&self, construct: &str, reason: &str, alternatives: &[&str]) -> Untranslatable {
        Untranslatable {
            construct: construct.to_string(),
            source: self.source.to_string(),
            target: self.target.to_string(),
            reason: reason.to_string(),
            alternatives: alternatives.iter().map(|s| s.to_string()).collect(),
        }
    }
}

type R<T> = Result<T, Untranslatable>;

/// What a target dialect supplies. Every method has a PostgreSQL default.
pub trait Target {
    fn name(&self) -> &'static str;

    fn quote_ident(&self, s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }

    fn string_literal(&self, s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    /// Placeholder text for the 1-based parameter `n`.
    fn placeholder(&self, n: u32) -> String {
        format!("${n}")
    }

    fn bool_literal(&self, b: bool, _ctx: &mut Ctx) -> R<String> {
        Ok(if b { "TRUE" } else { "FALSE" }.to_string())
    }

    /// `lhs [NOT] LIKE rhs` / `ILIKE`, operands already printed.
    fn like(&self, lhs: &str, rhs: &str, negated: bool, ci: bool, ctx: &mut Ctx) -> R<String> {
        let kw = if ci { "ILIKE" } else { "LIKE" };
        ctx.note(if ci { "ilike" } else { "like" }, Tier::Exact, "", None)?;
        Ok(format!("{lhs} {}{kw} {rhs}", if negated { "NOT " } else { "" }))
    }

    /// A function call with arguments already printed. Return the full call text.
    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        ctx.note(format!("fn:{}", f.name), Tier::Exact, "", None)?;
        Ok(generic_call(&f.name, args, f.distinct, f.star))
    }

    fn cast(&self, expr: &str, ty: &SqlType, ctx: &mut Ctx) -> R<String> {
        ctx.note("cast", Tier::Exact, "", None)?;
        Ok(format!("CAST({expr} AS {})", pg_type_name(ty)))
    }

    fn distinct_on(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("distinct_on", Tier::Exact, "", None)
    }

    fn row_lock(&self, _lock: RowLock, ctx: &mut Ctx) -> R<()> {
        ctx.note("row_lock", Tier::Exact, "", None)
    }

    fn returning(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "", None)
    }

    fn upsert(&self, has_where: bool, ctx: &mut Ctx) -> R<()> {
        ctx.note(if has_where { "upsert_where" } else { "upsert" }, Tier::Exact, "", None)
    }

    /// `LIMIT`/`OFFSET` tail; operands already printed. Returns `""` when neither is present.
    fn limit_offset(&self, limit: Option<&str>, offset: Option<&str>, ctx: &mut Ctx) -> R<String> {
        let mut s = String::new();
        if let Some(l) = limit {
            s.push_str(&format!(" LIMIT {l}"));
            ctx.note("limit", Tier::Exact, "", None)?;
        }
        if let Some(o) = offset {
            s.push_str(&format!(" OFFSET {o}"));
            ctx.note("offset", Tier::Exact, "", None)?;
        }
        Ok(s)
    }

    /// Tier of a join kind on this target (SQLite lacks RIGHT/FULL before 3.39).
    fn join_tier(&self, _kind: JoinKind) -> Tier {
        Tier::Exact
    }

    /// Tier of `UPDATE .. FROM`.
    fn update_from_tier(&self) -> Tier {
        Tier::Exact
    }

    /// Hook for `[NOT] BETWEEN`. `numeric` is true when an operand is syntactically numeric (so no
    /// text collation is involved). Default: exact.
    fn between(&self, _numeric: bool, _ctx: &mut Ctx) -> R<()> {
        Ok(())
    }

    /// Hook for operators whose meaning differs. `numeric` is true when an operand is syntactically
    /// numeric (a number literal, arithmetic, or a numeric aggregate). Default: exact.
    fn binary_op(&self, _op: BinaryOp, _numeric: bool, _ctx: &mut Ctx) -> R<()> {
        Ok(())
    }

    /// Text for one ORDER BY item's null-ordering suffix, `""` for none. `nulls_first` is always
    /// explicit after reading (the reader fills in the source dialect's default).
    fn nulls_order(&self, desc: bool, nulls_first: bool, ctx: &mut Ctx) -> R<String> {
        // PostgreSQL default: NULLs are the largest value (last ascending, first descending).
        let default_first = desc;
        ctx.note("null_ordering", Tier::Exact, "", None)?;
        if nulls_first == default_first {
            Ok(String::new())
        } else {
            Ok(if nulls_first { " NULLS FIRST" } else { " NULLS LAST" }.to_string())
        }
    }
}

pub fn generic_call(name: &str, args: &[String], distinct: bool, star: bool) -> String {
    if star {
        return format!("{}(*)", name.to_uppercase());
    }
    format!(
        "{}({}{})",
        name.to_uppercase(),
        if distinct { "DISTINCT " } else { "" },
        args.join(", ")
    )
}

/// PostgreSQL's spelling of a type.
pub fn pg_type_name(ty: &SqlType) -> String {
    match ty {
        SqlType::SmallInt => "SMALLINT".into(),
        SqlType::Integer => "INTEGER".into(),
        SqlType::BigInt => "BIGINT".into(),
        SqlType::Real => "REAL".into(),
        SqlType::Double => "DOUBLE PRECISION".into(),
        SqlType::Numeric { precision: None, .. } => "NUMERIC".into(),
        SqlType::Numeric { precision: Some(p), scale: None } => format!("NUMERIC({p})"),
        SqlType::Numeric { precision: Some(p), scale: Some(s) } => format!("NUMERIC({p}, {s})"),
        SqlType::Text => "TEXT".into(),
        SqlType::Varchar(None) => "VARCHAR".into(),
        SqlType::Varchar(Some(n)) => format!("VARCHAR({n})"),
        SqlType::Boolean => "BOOLEAN".into(),
        SqlType::Date => "DATE".into(),
        SqlType::Timestamp { with_tz: false } => "TIMESTAMP".into(),
        SqlType::Timestamp { with_tz: true } => "TIMESTAMPTZ".into(),
        SqlType::Uuid => "UUID".into(),
        SqlType::Bytes => "BYTEA".into(),
        SqlType::Json => "JSON".into(),
        SqlType::Jsonb => "JSONB".into(),
    }
}
