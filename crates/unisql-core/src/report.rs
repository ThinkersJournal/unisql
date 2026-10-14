// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tiers, allowances, the per-translation report, and the refusal error.

use std::collections::BTreeSet;
use std::fmt;

/// How a target gives a construct. Order matters: a statement's tier is the maximum of its parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Same meaning by construction and by test.
    Exact,
    /// Target lacks it; an equivalent form exists, with a stated difference.
    Emulated,
    /// Same meaning only if the caller accepts a named assumption ([`Allowance`]).
    Conditional,
    /// No meaning-preserving equivalent. The writer returns [`Untranslatable`], never text.
    Refused,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Exact => "exact",
            Tier::Emulated => "emulated",
            Tier::Conditional => "conditional",
            Tier::Refused => "refused",
        }
    }
}

/// A named, explicit assumption a caller accepts for a Conditional construct.
/// There is **no** blanket "best effort" switch (DESIGN.md section 3).
///
/// **A name and the text above it are a contract.** Matrix files carry `<construct>.<allowance>` as a
/// leaf id, and `docs/LEAF-CONTRACTS.tsv` says that leaf means "the base leaf, with the divergence
/// this text describes accepted". Rewording a variant's doc is therefore a change of meaning: mint a
/// new variant (a new name) and leave the old one alone; ids are append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Allowance {
    /// `now()` yields text (`YYYY-MM-DD HH:MM:SS`, UTC) rather than a timestamptz.
    NowAsText,
    /// `CURRENT_DATE` yields text (`YYYY-MM-DD`, the UTC date) rather than a date in the session time
    /// zone.
    DateAsText,
    /// `LIKE` case sensitivity is a *connection setting* on the target, not a property of the
    /// statement; the source's case-sensitive `LIKE` is exact only if that setting is on.
    LikeCaseSensitivityIsConnectionSetting,
    /// Case folding (`ILIKE`, `lower`, `upper`) is ASCII-only on the target.
    AsciiOnlyCaseFolding,
    /// A bad `CAST` that errors on the source silently yields 0 / a numeric prefix on the target.
    LenientCast,
    /// Division or modulo by zero is an error on the source and `NULL` on the target.
    DivisionByZeroIsNull,
    /// Text ordering and comparison use the target's collation; both engines must be configured
    /// alike (for example byte order on both) for results to match.
    TextCollation,
    /// Boolean values are stored and returned as integers 0/1.
    BooleanAsInteger,
    /// Timestamp arithmetic goes through text (`'YYYY-MM-DD HH:MM:SS'`, UTC, whole seconds).
    TimestampArithmeticAsText,
    /// JSON `->>` yields text on the source; on the target a scalar's type affinity can survive
    /// (a JSON `true` extracts as 1, not 'true').
    JsonScalarAffinity,
    /// JSON `->` yields a JSON value on the source; on the target it is its text representation, so
    /// comparison is textual (whitespace and key order are not normalised).
    JsonRepresentationAsText,
    /// The source `/` is exact (DECIMAL) division; the target divides as double precision.
    DecimalDivisionAsFloat,
    /// An array parameter used for membership: a `NULL` array, `NULL` elements or a `NULL` operand
    /// give an unknown result on the source but a definite true/false on the target.
    ArrayNullSemantics,
    /// `a / b` becomes integer division (`DIV`), valid only if both operands are integers.
    IntegerDivisionAssumed,
    /// An upsert fires on any unique key of the target, not only the stated conflict target.
    UpsertAnyUniqueKey,
    /// An aggregated string is silently truncated at a server setting (`group_concat_max_len`).
    AggregateStringTruncation,
}

impl Allowance {
    /// Every named allowance (what a caller who accepts all assumptions would grant).
    pub const ALL: &[Allowance] = &[
        Allowance::NowAsText,
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
        Allowance::DecimalDivisionAsFloat,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Allowance::NowAsText => "now_as_text",
            Allowance::DateAsText => "date_as_text",
            Allowance::LikeCaseSensitivityIsConnectionSetting => {
                "like_case_sensitivity_is_connection_setting"
            }
            Allowance::AsciiOnlyCaseFolding => "ascii_only_case_folding",
            Allowance::LenientCast => "lenient_cast",
            Allowance::DivisionByZeroIsNull => "division_by_zero_is_null",
            Allowance::TextCollation => "text_collation",
            Allowance::BooleanAsInteger => "boolean_as_integer",
            Allowance::TimestampArithmeticAsText => "timestamp_arithmetic_as_text",
            Allowance::JsonScalarAffinity => "json_scalar_affinity",
            Allowance::JsonRepresentationAsText => "json_representation_as_text",
            Allowance::ArrayNullSemantics => "array_null_semantics",
            Allowance::DecimalDivisionAsFloat => "decimal_division_as_float",
            Allowance::IntegerDivisionAssumed => "integer_division_assumed",
            Allowance::UpsertAnyUniqueKey => "upsert_any_unique_key",
            Allowance::AggregateStringTruncation => "aggregate_string_truncation",
        }
    }
}

/// One line of the report: a construct used and how the target gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Stable construct id, e.g. `returning`, `upsert`, `ilike`, `fn:string_agg`.
    pub construct: String,
    pub tier: Tier,
    /// What differs, for Emulated/Conditional.
    pub note: String,
    pub allowance: Option<Allowance>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub entries: Vec<Entry>,
}

impl Report {
    pub fn push(&mut self, e: Entry) {
        if !self.entries.contains(&e) {
            self.entries.push(e);
        }
    }
    pub fn worst(&self) -> Tier {
        self.entries
            .iter()
            .map(|e| e.tier)
            .max()
            .unwrap_or(Tier::Exact)
    }
    pub fn constructs(&self) -> BTreeSet<&str> {
        self.entries.iter().map(|e| e.construct.as_str()).collect()
    }
    pub fn tier_of(&self, construct: &str) -> Option<Tier> {
        self.entries
            .iter()
            .filter(|e| e.construct == construct)
            .map(|e| e.tier)
            .max()
    }
}

/// Everything a caller may relax, explicitly.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub allow: BTreeSet<Allowance>,
}

impl Options {
    pub fn with(mut self, a: Allowance) -> Self {
        self.allow.insert(a);
        self
    }
}

/// The only way a translation fails. Carries no SQL text (DESIGN.md section 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Untranslatable {
    pub construct: String,
    pub source: String,
    pub target: String,
    pub reason: String,
    /// What the caller could do instead, if anything.
    pub alternatives: Vec<String>,
}

impl Untranslatable {
    /// The leaf id a gate should look up for this refusal. A refusal names the form that cannot be
    /// translated (`cast.to_uuid`, `upsert.no_conflict_key`) or, where the whole construct is
    /// refused, the construct itself (`fn:md5`). A backend that refuses a leaf has no enabled row
    /// for it.
    pub fn subject(&self) -> String {
        self.construct.clone()
    }
}

impl fmt::Display for Untranslatable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "untranslatable `{}` ({} -> {}): {}",
            self.construct, self.source, self.target, self.reason
        )?;
        if !self.alternatives.is_empty() {
            write!(f, " [alternatives: {}]", self.alternatives.join("; "))?;
        }
        Ok(())
    }
}

impl std::error::Error for Untranslatable {}

/// Reading failed: the text is not valid for the source dialect, or uses a construct the IR does not
/// represent (which is an error, not a pass-through).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    Syntax(String),
    NotInIr { construct: String, detail: String },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Syntax(s) => write!(f, "syntax error: {s}"),
            ReadError::NotInIr { construct, detail } => {
                write!(f, "construct `{construct}` is not in the IR: {detail}")
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// How the caller must bind one source parameter on the target. The *text* of a statement is not
/// enough when a parameter changes shape between engines (an array that SQLite and MySQL can only
/// take as JSON text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamBinding {
    /// A single value, bound as it would be on the source.
    Scalar,
    /// A native array value (PostgreSQL `int4[]`, `text[]`, ...).
    NativeArray,
    /// A JSON array, bound as text: `[1,3]` or `["a","b"]`.
    JsonArrayText,
}

impl ParamBinding {
    pub fn name(self) -> &'static str {
        match self {
            ParamBinding::Scalar => "scalar",
            ParamBinding::NativeArray => "native_array",
            ParamBinding::JsonArrayText => "json_array_text",
        }
    }
}

/// A successful translation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translation {
    pub sql: String,
    /// Number of distinct source parameters (`$1..$n`).
    pub param_count: u32,
    /// Source parameter numbers in the order their placeholders appear in `sql`. A target with
    /// numbered placeholders (`$n`, `?n`) can ignore it; a target with positional `?` binds the
    /// values in exactly this order (a source parameter used twice appears twice).
    pub param_order: Vec<u32>,
    /// One entry per source parameter (index `n - 1` is `$n`): how to bind it on the target.
    pub param_bindings: Vec<ParamBinding>,
    pub report: Report,
}

/// An assumption the *source* text carries that the target must honour or refuse: for example a
/// SQLite `LIKE` is case-insensitive only while a connection setting is at its default. Recorded by
/// the reader and checked against [`Options`] like any Conditional construct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadNote {
    pub construct: String,
    pub note: String,
    pub allowance: Allowance,
}

/// A successful read.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    pub stmt: crate::ast::Statement,
    pub notes: Vec<ReadNote>,
}
