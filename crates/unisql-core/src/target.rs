// SPDX-License-Identifier: MIT OR Apache-2.0
//! The writer side of a dialect: a [`Target`] supplies *differences*; the shared printer in
//! [`crate::print`] does the rest. Defaults describe PostgreSQL, the first source dialect, so a
//! Postgres target is nearly empty and every other target overrides only what it must.

use crate::ast::*;
use crate::report::{Allowance, Entry, Options, ParamBinding, Report, Tier, Untranslatable};

/// Per-translation state handed to every hook.
pub struct Ctx<'a> {
    pub report: Report,
    pub opts: &'a Options,
    pub source: &'a str,
    pub target: &'a str,
    pub max_param: u32,
    pub param_order: Vec<u32>,
    /// True while printing the SET list of an upsert (so `excluded.c` can be spelled per target).
    pub in_upsert: bool,
    /// Printed `ORDER BY` of the aggregate call being printed, for the function hook to consume.
    /// A hook that does not `take()` it cannot honour the order, and the printer then refuses.
    pub agg_order: Option<String>,
    /// Parameters used as arrays, with the binding the target requires.
    pub array_params: std::collections::BTreeMap<u32, ParamBinding>,
    /// Parameters used as plain values (a parameter must not be both).
    pub scalar_params: std::collections::BTreeSet<u32>,
}

impl<'a> Ctx<'a> {
    pub fn new(opts: &'a Options, source: &'a str, target: &'a str) -> Self {
        Ctx {
            report: Report::default(),
            opts,
            source,
            target,
            max_param: 0,
            param_order: Vec::new(),
            in_upsert: false,
            agg_order: None,
            array_params: Default::default(),
            scalar_params: Default::default(),
        }
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
                // Same dialect in and out is the identity: no assumption changes, so none is gated.
                if self.source != self.target && !self.opts.allow.contains(&a) {
                    return Err(self.refuse(
                        &construct,
                        &format!("conditional: needs allowance `{}`: {}", a.name(), note),
                        &[],
                    ));
                }
                self.report.push(Entry {
                    construct,
                    tier,
                    note,
                    allowance: Some(a),
                });
                Ok(())
            }
            _ => {
                self.report.push(Entry {
                    construct,
                    tier,
                    note,
                    allowance,
                });
                Ok(())
            }
        }
    }

    /// Record that source parameter `n` is an array that the caller must bind as `binding`.
    pub fn bind_array_param(&mut self, n: u32, binding: ParamBinding) {
        self.array_params.insert(n, binding);
    }

    /// True for sources whose casts, division and comparisons are lenient or collation-dependent
    /// in ways PostgreSQL's are not.
    pub fn lenient_source(&self) -> bool {
        matches!(self.source, "sqlite" | "mysql" | "mariadb")
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

/// The kind of statement a clause belongs to (some clauses exist only for some kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StmtKind {
    Insert,
    Update,
    Delete,
}

/// The escape character of a `LIKE`, as the IR states it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape<'a> {
    /// No escape character at all (the SQLite default).
    None,
    /// A backslash (the PostgreSQL default).
    Default,
    /// An explicit printed expression.
    Custom(&'a str),
}

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
    fn like(
        &self,
        lhs: &str,
        rhs: &str,
        negated: bool,
        ci: bool,
        esc: Escape<'_>,
        ctx: &mut Ctx,
    ) -> R<String> {
        let kw = if ci { "ILIKE" } else { "LIKE" };
        ctx.note(if ci { "ilike" } else { "like" }, Tier::Exact, "", None)?;
        let tail = match esc {
            Escape::Default => String::new(), // PostgreSQL's default escape is a backslash
            Escape::None => " ESCAPE ''".to_string(),
            Escape::Custom(c) => format!(" ESCAPE {c}"),
        };
        Ok(format!(
            "{lhs} {}{kw} {rhs}{tail}",
            if negated { "NOT " } else { "" }
        ))
    }

    /// A function call with arguments already printed. Return the full call text.
    fn function(&self, f: &Function, args: &[String], ctx: &mut Ctx) -> R<String> {
        // Same dialect in and out is the identity: the reader accepted the name. Otherwise the name
        // must be on this target's verified list; a foreign function name is never passed through.
        if ctx.source != self.name() && !PORTABLE_FUNCTIONS.contains(&f.name.as_str()) {
            return Err(ctx.refuse(
                &format!("fn:{}", f.name),
                "no verified mapping for this function on this target yet",
                &[],
            ));
        }
        ctx.note(format!("fn:{}", f.name), Tier::Exact, "", None)?;
        let order = ctx.agg_order.take();
        Ok(generic_call_ordered(
            &f.name,
            args,
            f.distinct,
            f.star,
            order.as_deref(),
        ))
    }

    fn cast(&self, expr: &str, ty: &SqlType, ctx: &mut Ctx) -> R<String> {
        if ctx.lenient_source() {
            ctx.note(
                format!("cast.to_{}", ty.leaf()),
                Tier::Conditional,
                "the source casts leniently (a malformed value becomes 0); the target raises an error",
                Some(Allowance::LenientCast),
            )?;
        } else {
            ctx.note(format!("cast.to_{}", ty.leaf()), Tier::Exact, "", None)?;
        }
        Ok(format!("CAST({expr} AS {})", pg_type_name(ty)))
    }

    fn distinct_on(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("distinct_on", Tier::Exact, "", None)
    }

    fn row_lock(&self, _lock: RowLock, ctx: &mut Ctx) -> R<()> {
        ctx.note("row_lock", Tier::Exact, "", None)
    }

    fn returning(&self, _kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "", None)
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

    /// Whole-query rewrite before printing (for example `DISTINCT ON` via a window function).
    /// Return `Some(q2)` to print `q2` instead; `q2` must not trigger the same rewrite again.
    fn rewrite_query(&self, _q: &Query, _ctx: &mut Ctx) -> R<Option<Query>> {
        Ok(None)
    }

    /// A feature the printer is about to emit: `window_function`, `filter_clause`,
    /// `recursive_cte`, `named_arg`. Default: exact (PostgreSQL).
    fn feature(&self, name: &'static str, ctx: &mut Ctx) -> R<()> {
        ctx.note(name, Tier::Exact, "", None)
    }

    /// `TRIM(..)` with operands already printed.
    fn trim(&self, side: TrimSide, expr: &str, chars: Option<&str>, ctx: &mut Ctx) -> R<String> {
        ctx.note("trim", Tier::Exact, "", None)?;
        Ok(match (side, chars) {
            (TrimSide::Both, None) => format!("TRIM({expr})"),
            (s, c) => {
                let kw = match s {
                    TrimSide::Both => "BOTH",
                    TrimSide::Leading => "LEADING",
                    TrimSide::Trailing => "TRAILING",
                };
                match c {
                    Some(c) => format!("TRIM({kw} {c} FROM {expr})"),
                    None => format!("TRIM({kw} FROM {expr})"),
                }
            }
        })
    }

    /// `left (+|-) make_interval(parts..)`; `parts` are (unit, printed value). Return `Some(text)`
    /// to replace the whole expression, `None` to print it the generic way.
    fn interval_arith(
        &self,
        _minus: bool,
        _left: &str,
        _parts: &[(String, String)],
        _ctx: &mut Ctx,
    ) -> R<Option<String>> {
        Ok(None)
    }

    /// JSON `->` / `->>` with operands printed. Default: PostgreSQL.
    fn json_access(&self, text: bool, lhs: &str, rhs: &str, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            if text { "json_get_text" } else { "json_get" },
            Tier::Exact,
            "",
            None,
        )?;
        Ok(format!("{lhs} {} {rhs}", if text { "->>" } else { "->" }))
    }

    /// Replace the default `l OP r` text for a binary operator (for example `||` -> `CONCAT`,
    /// `/` -> `DIV`). Operands are already printed. Return `None` for the default.
    fn binary_override(
        &self,
        _op: BinaryOp,
        _l: &str,
        _r: &str,
        _numeric: bool,
        _ctx: &mut Ctx,
    ) -> R<Option<String>> {
        Ok(None)
    }

    /// One ORDER BY item with its direction and NULL placement. `nulls_first` is explicit.
    fn order_item(&self, expr: &str, desc: bool, nulls_first: bool, ctx: &mut Ctx) -> R<String> {
        let mut s = expr.to_string();
        if desc {
            s.push_str(" DESC");
        }
        s.push_str(&self.nulls_order(desc, nulls_first, ctx)?);
        Ok(s)
    }

    /// How the SET list of an upsert refers to the value that was proposed for insertion.
    fn excluded_ref(&self, column_quoted: &str) -> String {
        format!("EXCLUDED.{column_quoted}")
    }

    /// The whole `ON CONFLICT ..` clause. `set` and `where_` are printed; `columns` are quoted.
    #[allow(clippy::too_many_arguments)]
    fn upsert_clause(
        &self,
        conflict_cols: &[String],
        do_nothing: bool,
        set: &str,
        where_: Option<&str>,
        _first_insert_col: &str,
        ctx: &mut Ctx,
    ) -> R<String> {
        if !do_nothing && conflict_cols.is_empty() {
            return Err(ctx.refuse(
                "upsert.no_conflict_key",
                "the source upsert names no conflict key (MySQL ON DUPLICATE KEY UPDATE fires on any unique key); this target needs the key columns",
                &["state the conflict key columns in the statement"],
            ));
        }
        ctx.note(
            if where_.is_some() {
                "upsert_where"
            } else {
                "upsert"
            },
            Tier::Exact,
            "",
            None,
        )?;
        let mut s = String::from(" ON CONFLICT");
        if !conflict_cols.is_empty() {
            s.push_str(&format!(" ({})", conflict_cols.join(", ")));
        }
        if do_nothing {
            s.push_str(" DO NOTHING");
        } else {
            s.push_str(&format!(" DO UPDATE SET {set}"));
            if let Some(w) = where_ {
                s.push_str(&format!(" WHERE {w}"));
            }
        }
        Ok(s)
    }

    /// Membership in an array parameter: `lhs = ANY(ph)` (or `<> ALL` when `negated`). The target
    /// must record how the caller binds parameter `n` via [`Ctx::bind_array_param`]. **The default
    /// refuses**: a target with no array codec has no honest translation.
    fn array_membership(
        &self,
        _lhs: &str,
        _ph: &str,
        _negated: bool,
        n: u32,
        ctx: &mut Ctx,
    ) -> R<String> {
        Err(ctx.refuse(
            "array_param",
            &format!(
                "target `{}` has no codec for an array parameter (${n})",
                ctx.target
            ),
            &["expand the array into an IN (..) list in the application"],
        ))
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
            Ok(if nulls_first {
                " NULLS FIRST"
            } else {
                " NULLS LAST"
            }
            .to_string())
        }
    }
}

/// Functions with the same name and meaning on every dialect UniSQL targets so far.
pub const PORTABLE_FUNCTIONS: &[&str] = &[
    "count",
    "sum",
    "min",
    "max",
    "avg",
    "coalesce",
    "nullif",
    "abs",
    "lower",
    "upper",
    "length",
    "replace",
    "substr",
    "row_number",
    "rank",
    "dense_rank",
    "percent_rank",
    "cume_dist",
    "ntile",
    "lag",
    "lead",
    "first_value",
    "last_value",
    "nth_value",
];

pub fn generic_call(name: &str, args: &[String], distinct: bool, star: bool) -> String {
    generic_call_ordered(name, args, distinct, star, None)
}

/// `NAME([DISTINCT] args [ORDER BY ..])`.
pub fn generic_call_ordered(
    name: &str,
    args: &[String],
    distinct: bool,
    star: bool,
    order: Option<&str>,
) -> String {
    let ord = order.map(|o| format!(" ORDER BY {o}")).unwrap_or_default();
    if star {
        return format!("{}(*)", name.to_uppercase());
    }
    format!(
        "{}({}{}{})",
        name.to_uppercase(),
        if distinct { "DISTINCT " } else { "" },
        args.join(", "),
        ord
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
        SqlType::Numeric {
            precision: None, ..
        } => "NUMERIC".into(),
        SqlType::Numeric {
            precision: Some(p),
            scale: None,
        } => format!("NUMERIC({p})"),
        SqlType::Numeric {
            precision: Some(p),
            scale: Some(s),
        } => format!("NUMERIC({p}, {s})"),
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
