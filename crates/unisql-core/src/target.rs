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
    /// The tables in scope while printing, for column classes from `opts.schema`.
    pub scopes: crate::schema::Scopes<'a>,
    pub source: &'a str,
    pub target: &'a str,
    pub max_param: u32,
    pub param_order: Vec<u32>,
    /// True while printing the SET list of an upsert (so `excluded.c` can be spelled per target).
    pub in_upsert: bool,
    /// The printed (last-part) name of the INSERT's table while `in_upsert`: an unqualified column in
    /// a MySQL-written SET list names the existing row, which PostgreSQL (ambiguous with `EXCLUDED`)
    /// needs spelled `table.col`.
    pub(crate) upsert_table: Option<String>,
    /// Printed `ORDER BY` of the aggregate call being printed, for the function hook to consume.
    /// A hook that does not `take()` it cannot honour the order, and the printer then refuses.
    pub agg_order: Option<String>,
    /// True while the ORDER BY item being printed is a column the schema says is not text (bytes,
    /// numbers, dates): its ordering involves no text collation, so the target hook notes it `Exact`.
    pub order_non_text: bool,
    /// True while a `make_interval(unit => n)` call is printed as the right side of `ts +/- ..`: the
    /// named notation is then only how PostgreSQL spells the interval, not a `named_arg` need.
    pub interval_notation: bool,
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
            scopes: crate::schema::Scopes::new(opts.schema.as_ref()),
            source,
            target,
            max_param: 0,
            param_order: Vec::new(),
            in_upsert: false,
            upsert_table: None,
            agg_order: None,
            order_non_text: false,
            interval_notation: false,
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
/// A target's way of writing `UPDATE|DELETE .. ORDER BY .. LIMIT n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmlLimit {
    /// Trailing `ORDER BY .. LIMIT n` on the statement itself.
    Native,
    /// `WHERE <row id> IN (SELECT <row id> FROM t WHERE .. ORDER BY .. LIMIT n)`.
    RowIdSubquery(&'static str),
}

/// A target's way of writing the extra tables of a multi-table `UPDATE` or `DELETE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmlSources {
    /// `UPDATE t SET .. FROM f WHERE ..` and `DELETE FROM t USING f WHERE ..` (PostgreSQL).
    FromClause,
    /// `UPDATE t, f SET .. WHERE ..` and `DELETE t FROM t, f WHERE ..` (MySQL).
    Join,
    /// `UPDATE .. FROM` only; no `DELETE .. USING` (SQLite).
    UpdateFromOnly,
}

pub trait Target {
    fn name(&self) -> &'static str;

    fn quote_ident(&self, s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }

    fn string_literal(&self, s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    /// A byte-string literal. PostgreSQL has no hex literal of this type, so it casts an escape string.
    fn bytes_literal(&self, b: &[u8]) -> String {
        let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
        format!("'\\x{hex}'::bytea")
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

    /// `lhs [NOT] REGEXP rhs`, operands already printed. The default is PostgreSQL's `~` / `!~`.
    fn regexp(&self, lhs: &str, rhs: &str, negated: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(
            "regexp",
            Tier::Conditional,
            "regular-expression syntax and case sensitivity differ between engines; exact only for \
             patterns in the common subset",
            Some(Allowance::RegexpFlavor),
        )?;
        Ok(format!("{lhs} {} {rhs}", if negated { "!~" } else { "~" }))
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

    /// The body of a `GROUP BY` with `WITH ROLLUP`, keys printed. Default (PostgreSQL):
    /// `ROLLUP (a, b)`, exact.
    fn group_by_rollup(&self, keys: &[String], ctx: &mut Ctx) -> R<String> {
        ctx.note("group_by.rollup", Tier::Exact, "", None)?;
        Ok(format!("ROLLUP ({})", keys.join(", ")))
    }

    /// `a <=> b` (null-safe equality), operands printed. Default (PostgreSQL): `IS NOT DISTINCT FROM`.
    fn null_safe_eq(&self, lhs: &str, rhs: &str, ctx: &mut Ctx) -> R<String> {
        ctx.note("null_safe_eq", Tier::Exact, "", None)?;
        Ok(format!("{lhs} IS NOT DISTINCT FROM {rhs}"))
    }

    /// `EXTRACT(field FROM expr)`, operand printed. Default (PostgreSQL): exact, one leaf per field
    /// (`extract.year`, `extract.epoch`, ...): a target maps a field only once it is verified.
    fn extract(&self, field: &str, expr: &str, interval: bool, ctx: &mut Ctx) -> R<String> {
        ctx.note(extract_leaf(field, interval), Tier::Exact, "", None)?;
        Ok(format!("EXTRACT({} FROM {expr})", field.to_uppercase()))
    }

    /// `EXTRACT(EPOCH FROM a - b)`, operands printed: the seconds between two timestamps.
    /// `None` (the default) means "no verified mapping": the printer then goes the generic way and
    /// the `extract` hook decides (PostgreSQL writes it back, the others refuse).
    fn epoch_of_difference(&self, _a: &str, _b: &str, _ctx: &mut Ctx) -> R<Option<String>> {
        Ok(None)
    }

    /// `EXTRACT(EPOCH FROM clock_timestamp())`: the wall clock at the call, in seconds since the epoch
    /// with the fraction. The default refuses; each target states how it reads its clock.
    fn epoch_of_clock(&self, ctx: &mut Ctx) -> R<String> {
        Err(ctx.refuse(
            "extract.epoch",
            "no mapping for the per-call wall clock",
            &["bind the time as a parameter"],
        ))
    }

    /// `ARRAY[..]`, elements printed. Default (PostgreSQL): exact; a target without an array type
    /// refuses the leaf `array_literal`.
    fn array_literal(&self, items: &[String], ctx: &mut Ctx) -> R<String> {
        ctx.note("array_literal", Tier::Exact, "", None)?;
        Ok(format!("ARRAY[{}]", items.join(", ")))
    }

    /// A table-valued function in FROM (`generate_series`, `jsonb_object_keys`, `unnest`, ...). Default
    /// (PostgreSQL): exact, one leaf per function (`table_function.<name>`). A target that has not
    /// verified the function refuses that leaf.
    fn table_function(&self, name: &str, ctx: &mut Ctx) -> R<()> {
        ctx.note(format!("table_function.{name}"), Tier::Exact, "", None)
    }

    /// Whether `FROM generate_series(a, b[, step])` with integer literal arguments and an alias may be
    /// written as a derived table over a recursive CTE. Default (PostgreSQL): no, it is native there.
    fn series_as_recursive_cte(&self) -> bool {
        false
    }

    /// `expr AT TIME ZONE zone`. Default (PostgreSQL): exact. A target without it refuses the leaf
    /// `at_time_zone`.
    fn at_time_zone(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("at_time_zone", Tier::Exact, "", None)
    }

    /// `ARRAY(SELECT ..)`. Default (PostgreSQL): exact. A target without an array type refuses the
    /// leaf `array_subquery`.
    fn array_subquery(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("array_subquery", Tier::Exact, "", None)
    }

    /// `WITH .. INSERT | UPDATE | DELETE ..` (the common table expressions precede the modifying
    /// statement). The default (PostgreSQL) has all three; each is its own leaf: `with.insert`,
    /// `with.update`, `with.delete`.
    fn with_statement(&self, kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        ctx.note(with_leaf(kind), Tier::Exact, "", None)
    }

    /// `WITH x AS (INSERT/UPDATE/DELETE .. RETURNING ..) ..`. The default (PostgreSQL) has it; a
    /// target without it refuses the leaf `cte.data_modifying`.
    fn data_modifying_cte(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("cte.data_modifying", Tier::Exact, "", None)
    }

    /// A row-locking clause. The default (PostgreSQL) gives every form; each form is its own leaf:
    /// `row_lock` (FOR UPDATE), `row_lock.share`, `row_lock.of`, `row_lock.nowait`,
    /// `row_lock.skip_locked`.
    fn row_lock(&self, lock: &RowLockClause, ctx: &mut Ctx) -> R<()> {
        note_row_lock_leaves(lock, ctx)
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

    /// How the extra tables of a multi-table `UPDATE` or `DELETE` are written.
    fn dml_sources(&self) -> DmlSources {
        DmlSources::FromClause
    }

    /// How `UPDATE|DELETE .. ORDER BY .. LIMIT n` is written. The default (PostgreSQL) has no
    /// direct form: the row is picked by its physical identifier in a subquery.
    fn dml_limit(&self) -> DmlLimit {
        DmlLimit::RowIdSubquery("ctid")
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

    /// The native spelling of an insert that replaces conflicting rows (`REPLACE INTO` on MySQL and
    /// MariaDB, `INSERT OR REPLACE INTO` on SQLite). `None` (the default, PostgreSQL) means there is
    /// none: the printer then writes an upsert when the schema states the table's columns and its
    /// one unique key, and refuses (leaf `replace_into`) otherwise.
    fn replace_keyword(&self) -> Option<&'static str> {
        None
    }

    /// Whether an upsert must name its conflict key columns (PostgreSQL and SQLite do; MySQL and
    /// MariaDB fire on any unique key). When it does and the source stated none, the printer takes
    /// the key from the schema if the table has exactly one stated unique key.
    fn needs_conflict_key(&self) -> bool {
        true
    }

    /// How the SET list of an upsert refers to the value that was proposed for insertion.
    fn excluded_ref(&self, column_quoted: &str) -> String {
        format!("EXCLUDED.{column_quoted}")
    }

    /// Whether an upsert's `WHERE` has no native spelling and is written into the `SET` expressions as
    /// `col = IF(cond, new, col)` (MySQL and MariaDB). The printer does the rewrite.
    fn upsert_where_as_if(&self) -> bool {
        false
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
                "the source upsert names no conflict key (MySQL ON DUPLICATE KEY UPDATE fires on any unique key); this target needs the key columns, and the schema does not state exactly one unique key for the table",
                &["state the conflict key columns in the statement", "or give the schema the table's unique key (Schema::with_unique_key, or schema_from_ddl)"],
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
    // `strpos(haystack, needle)`: the IR name for MySQL `LOCATE`/`INSTR`, `POSITION(.. IN ..)` and
    // SQLite `instr`; each writer spells it natively or refuses.
    "strpos",
    // `ceil` / `floor` of a number (the reader normalises CEILING).
    "ceil",
    "floor",
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
        SqlType::Regclass => "REGCLASS".into(),
        SqlType::Bytes => "BYTEA".into(),
        SqlType::Json => "JSON".into(),
        SqlType::Jsonb => "JSONB".into(),
    }
}

/// The argument at `i` when it is a non-negative integer literal.
pub fn literal_count(f: &Function, i: usize) -> Option<u64> {
    match f.args.get(i) {
        Some(FunctionArg::Expr(Expr::Literal(Literal::Number(n)))) => n.parse().ok(),
        _ => None,
    }
}

/// The argument at `i` when it is a non-empty string literal.
pub fn literal_nonempty_string(f: &Function, i: usize) -> bool {
    matches!(
        f.args.get(i),
        Some(FunctionArg::Expr(Expr::Literal(Literal::String(s)))) if !s.is_empty()
    )
}

/// The fractional-second precision written in `UTC_TIMESTAMP(p)`: `Some(0)` when absent, `None`
/// when it is not an integer literal.
pub fn utc_timestamp_precision(f: &Function) -> Option<u32> {
    match f.args.as_slice() {
        [] => Some(0),
        [FunctionArg::Expr(Expr::Literal(Literal::Number(n)))] => n.parse().ok(),
        _ => None,
    }
}

/// `date_trunc('<field>', x)` with a literal field: that field, lower-cased.
pub fn date_trunc_field(f: &Function) -> Option<String> {
    if f.name != "date_trunc" {
        return None;
    }
    match f.args.first() {
        Some(FunctionArg::Expr(Expr::Literal(Literal::String(s)))) => Some(s.to_ascii_lowercase()),
        _ => None,
    }
}

/// The leaf of `EXTRACT(field FROM x)`: `extract.<field>`, and `extract.epoch.interval` when the operand
/// is a difference of timestamps (an interval), which is not the epoch of a timestamp.
pub fn extract_leaf(field: &str, interval: bool) -> String {
    if interval && field == "epoch" {
        "extract.epoch.interval".to_string()
    } else {
        format!("extract.{field}")
    }
}

/// The base leaf of a locking clause: `row_lock` for FOR UPDATE, `row_lock.share` for FOR SHARE.
pub fn row_lock_leaf(lock: &RowLockClause) -> &'static str {
    match lock.strength {
        RowLock::Update => "row_lock",
        RowLock::Share => "row_lock.share",
    }
}

/// Records every leaf a locking clause uses (the same set `needs::constructs` names), all Exact.
pub fn note_row_lock_leaves(lock: &RowLockClause, ctx: &mut Ctx) -> R<()> {
    ctx.note(row_lock_leaf(lock), Tier::Exact, "", None)?;
    if lock.of.is_some() {
        ctx.note("row_lock.of", Tier::Exact, "", None)?;
    }
    match lock.wait {
        LockWait::Block => {}
        LockWait::NoWait => ctx.note("row_lock.nowait", Tier::Exact, "", None)?,
        LockWait::SkipLocked => ctx.note("row_lock.skip_locked", Tier::Exact, "", None)?,
    }
    Ok(())
}

/// The leaf of `WITH .. <statement>`.
pub fn with_leaf(kind: StmtKind) -> &'static str {
    match kind {
        StmtKind::Insert => "with.insert",
        StmtKind::Update => "with.update",
        StmtKind::Delete => "with.delete",
    }
}
