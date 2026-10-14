// SPDX-License-Identifier: MIT OR Apache-2.0
//! The UniSQL intermediate representation.
//!
//! Dialect-neutral nodes. **A construct that is not representable here is an error at read time,
//! never a pass-through** (DESIGN.md section 2.3). No type from any parser crate appears in this
//! module's public surface, so the parser behind a dialect's reader is replaceable.

/// A possibly-qualified name, parts in order (`schema.table` is two parts). Parts are *unquoted*
/// identifiers; quoting is the writer's decision.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Name(pub Vec<String>);

impl Name {
    pub fn simple(s: impl Into<String>) -> Self {
        Name(vec![s.into()])
    }
    pub fn last(&self) -> &str {
        self.0.last().map(String::as_str).unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Query(Box<Query>),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub with: Vec<Cte>,
    /// `WITH RECURSIVE`
    pub recursive: bool,
    pub body: SetExpr,
    pub order_by: Vec<OrderItem>,
    pub limit: Option<Expr>,
    pub offset: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cte {
    pub name: String,
    pub columns: Vec<String>,
    pub query: Query,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SetExpr {
    Select(Box<Select>),
    SetOp {
        op: SetOp,
        all: bool,
        left: Box<SetExpr>,
        right: Box<SetExpr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOp {
    Union,
    Intersect,
    Except,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Distinct {
    No,
    Yes,
    /// PostgreSQL `DISTINCT ON (..)`: a construct, so the SQLite writer must emulate or refuse it.
    On(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub distinct: Distinct,
    pub projection: Vec<SelectItem>,
    pub from: Vec<TableRef>,
    pub where_: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub having: Option<Expr>,
    pub lock: Option<RowLock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowLock {
    Update,
    Share,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectItem {
    Expr { expr: Expr, alias: Option<String> },
    Wildcard,
    QualifiedWildcard(Name),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TableRef {
    Table {
        name: Name,
        alias: Option<String>,
    },
    Subquery {
        query: Box<Query>,
        alias: String,
    },
    Join {
        left: Box<TableRef>,
        right: Box<TableRef>,
        kind: JoinKind,
        on: Option<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderItem {
    pub expr: Expr,
    pub desc: bool,
    /// `None` = the source did not say. The default differs per engine (DESIGN.md section 3.1), so
    /// a writer must not guess when it matters.
    pub nulls_first: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub table: Name,
    pub columns: Vec<String>,
    pub source: InsertSource,
    pub on_conflict: Option<OnConflict>,
    pub returning: Vec<SelectItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InsertSource {
    Values(Vec<Vec<Expr>>),
    Query(Box<Query>),
    DefaultValues,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OnConflict {
    pub target: Vec<String>,
    pub action: ConflictAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConflictAction {
    DoNothing,
    DoUpdate {
        set: Vec<Assignment>,
        where_: Option<Expr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub column: String,
    pub value: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub table: Name,
    pub alias: Option<String>,
    pub set: Vec<Assignment>,
    pub from: Vec<TableRef>,
    pub where_: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Delete {
    pub table: Name,
    pub alias: Option<String>,
    pub using: Vec<TableRef>,
    pub where_: Option<Expr>,
    pub returning: Vec<SelectItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// `a`, `t.a`, `s.t.a`
    Column(Name),
    Literal(Literal),
    /// Positional parameter, **1-based** and dialect-neutral. A writer chooses `$n` or `?n`.
    Param(u32),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
    },
    InSubquery {
        expr: Box<Expr>,
        query: Box<Query>,
        negated: bool,
    },
    Between {
        expr: Box<Expr>,
        low: Box<Expr>,
        high: Box<Expr>,
        negated: bool,
    },
    /// `escape`: `None` = no escape character; `Some(literal "\\")` = the PostgreSQL default,
    /// which a reader makes explicit so no engine's default leaks through the IR.
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
        negated: bool,
        case_insensitive: bool,
        escape: Option<Box<Expr>>,
    },
    Case {
        operand: Option<Box<Expr>>,
        branches: Vec<(Expr, Expr)>,
        else_: Option<Box<Expr>>,
    },
    Cast {
        expr: Box<Expr>,
        ty: SqlType,
    },
    Function(Function),
    Subquery(Box<Query>),
    Exists {
        query: Box<Query>,
        negated: bool,
    },
    Nested(Box<Expr>),
    /// `expr = ANY($n)` (or `expr <> ALL($n)` when `negated`): membership in an *array parameter*.
    /// The parameter's binding shape is target-specific; see `Translation::param_bindings`.
    AnyParam {
        expr: Box<Expr>,
        param: u32,
        negated: bool,
    },
    /// `TRIM([LEADING|TRAILING|BOTH] [chars FROM] expr)`; `chars` absent means whitespace.
    Trim {
        expr: Box<Expr>,
        side: TrimSide,
        chars: Option<Box<Expr>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// Lower-cased, unquoted.
    pub name: String,
    pub args: Vec<FunctionArg>,
    pub distinct: bool,
    /// `count(*)`
    pub star: bool,
    /// `FILTER (WHERE ..)` on an aggregate.
    pub filter: Option<Box<Expr>>,
    /// `OVER (..)`: makes the call a window function.
    pub over: Option<Window>,
    /// `agg(args ORDER BY ..)`: the order in which an aggregate consumes its input.
    pub order_by: Vec<OrderItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FunctionArg {
    Expr(Expr),
    /// `name => value` (PostgreSQL named notation).
    Named {
        name: String,
        value: Expr,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub partition_by: Vec<Expr>,
    pub order_by: Vec<OrderItem>,
    pub frame: Option<Frame>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub units: FrameUnits,
    pub start: FrameBound,
    pub end: Option<FrameBound>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameUnits {
    Rows,
    Range,
    Groups,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrameBound {
    CurrentRow,
    /// `UNBOUNDED PRECEDING` when `None`.
    Preceding(Option<Box<Expr>>),
    /// `UNBOUNDED FOLLOWING` when `None`.
    Following(Option<Box<Expr>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimSide {
    Both,
    Leading,
    Trailing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Null,
    Bool(bool),
    /// Integer or decimal literal kept as source text so no precision is lost in the IR.
    Number(String),
    String(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Minus,
    Plus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    /// `||`
    Concat,
    /// MySQL `DIV`: integer division (operands assumed integers on other targets).
    IntDivide,
    /// JSON `->` (returns JSON)
    JsonGet,
    /// JSON `->>` (returns text)
    JsonGetText,
}

/// Types, as the IR sees them. Precision/length arguments are kept; engines differ in what they do
/// with them, which is a Conditional-tier question for the writer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SqlType {
    SmallInt,
    Integer,
    BigInt,
    Real,
    Double,
    Numeric {
        precision: Option<u32>,
        scale: Option<u32>,
    },
    Text,
    Varchar(Option<u32>),
    Boolean,
    Date,
    Timestamp {
        with_tz: bool,
    },
    Uuid,
    Bytes,
    Json,
    Jsonb,
}

impl SqlType {
    /// The stable leaf name of this type, used in leaf ids such as `cast.to_uuid`. Precision and
    /// scale do not change the leaf: `varchar(10)` and `text` are one `cast.to_text`.
    pub fn leaf(&self) -> &'static str {
        match self {
            SqlType::SmallInt => "smallint",
            SqlType::Integer => "integer",
            SqlType::BigInt => "bigint",
            SqlType::Real => "real",
            SqlType::Double => "double",
            SqlType::Numeric { .. } => "numeric",
            SqlType::Text | SqlType::Varchar(_) => "text",
            SqlType::Boolean => "boolean",
            SqlType::Date => "date",
            SqlType::Timestamp { with_tz: false } => "timestamp",
            SqlType::Timestamp { with_tz: true } => "timestamptz",
            SqlType::Uuid => "uuid",
            SqlType::Bytes => "bytes",
            SqlType::Json => "json",
            SqlType::Jsonb => "jsonb",
        }
    }
}
