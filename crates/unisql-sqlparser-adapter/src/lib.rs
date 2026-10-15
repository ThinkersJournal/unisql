// SPDX-License-Identifier: MIT OR Apache-2.0
//! sqlparser adapter: text -> IR for the dialects that sqlparser can parse. The per-dialect
//! differences are the [`Rules`]; no parser type escapes this crate. **Anything the IR cannot represent is `ReadError::NotInIr`, never a pass-through.**
//!
//! `Query`, `Select`, `Insert`, `Update` and `Delete` are destructured with every field named and no
//! `..`: a sqlparser upgrade that adds a field fails the build here, forcing a decision rather than
//! silently dropping a construct.

use std::cell::RefCell;

use sqlparser::ast as sp;
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use unisql_core::ast::*;
use unisql_core::{Allowance, Column, ColumnClass, Read, ReadError, ReadNote, Schema};

/// What differs between source dialects at the AST-to-IR step.
pub struct Rules {
    pub name: &'static str,
    pub dialect: fn() -> Box<dyn Dialect>,
    /// Quoted identifiers are case-insensitive too (SQLite): fold everything to lower case.
    pub fold_quoted: bool,
    /// `NULL` sorts as the smallest value (SQLite) rather than the largest (PostgreSQL).
    pub nulls_smallest: bool,
    /// Parameters: `$n` (PostgreSQL) or `?`, `?n`, `:name`, `@name`, `$name` (SQLite).
    pub question_placeholders: bool,
    /// `LIKE` is case-insensitive by default (SQLite, ASCII only).
    pub like_ci_default: bool,
    /// `LIKE` has a default escape character (PostgreSQL: backslash); otherwise none.
    pub like_default_escape: bool,
    /// A negative `LIMIT` means no limit (SQLite).
    pub negative_limit_unlimited: bool,
}

pub static POSTGRES: Rules = Rules {
    name: "postgres",
    dialect: || Box::new(sqlparser::dialect::PostgreSqlDialect {}),
    fold_quoted: false,
    nulls_smallest: false,
    question_placeholders: false,
    like_ci_default: false,
    like_default_escape: true,
    negative_limit_unlimited: false,
};

pub static MYSQL: Rules = Rules {
    name: "mysql",
    dialect: || Box::new(sqlparser::dialect::MySqlDialect {}),
    fold_quoted: true,
    nulls_smallest: true,
    question_placeholders: true,
    like_ci_default: false,
    like_default_escape: true,
    negative_limit_unlimited: false,
};

pub static SQLITE: Rules = Rules {
    name: "sqlite",
    dialect: || Box::new(sqlparser::dialect::SQLiteDialect {}),
    fold_quoted: true,
    nulls_smallest: true,
    question_placeholders: true,
    like_ci_default: true,
    like_default_escape: false,
    negative_limit_unlimited: true,
};

struct Cx {
    rules: &'static Rules,
    max_param: u32,
    names: Vec<(String, u32)>,
    notes: Vec<ReadNote>,
}

thread_local! {
    static CX: RefCell<Option<Cx>> = const { RefCell::new(None) };
}

fn rules() -> &'static Rules {
    CX.with(|c| c.borrow().as_ref().expect("reader context").rules)
}

fn add_note(construct: &str, note: &str, allowance: Allowance) {
    CX.with(|c| {
        let mut b = c.borrow_mut();
        let cx = b.as_mut().expect("reader context");
        let n = ReadNote {
            construct: construct.into(),
            note: note.into(),
            allowance,
        };
        if !cx.notes.contains(&n) {
            cx.notes.push(n);
        }
    });
}

fn bump_param(explicit: Option<u32>, name: Option<&str>) -> u32 {
    CX.with(|c| {
        let mut b = c.borrow_mut();
        let cx = b.as_mut().expect("reader context");
        match (explicit, name) {
            (Some(n), _) => {
                cx.max_param = cx.max_param.max(n);
                n
            }
            (None, Some(nm)) => {
                if let Some((_, n)) = cx.names.iter().find(|(k, _)| k == nm) {
                    return *n;
                }
                cx.max_param += 1;
                let n = cx.max_param;
                cx.names.push((nm.to_string(), n));
                n
            }
            (None, None) => {
                cx.max_param += 1;
                cx.max_param
            }
        }
    })
}

type R<T> = Result<T, ReadError>;

fn no<T>(construct: &str, detail: &str) -> R<T> {
    Err(ReadError::NotInIr {
        construct: construct.to_string(),
        detail: detail.to_string(),
    })
}

/// Read exactly one statement written in the dialect described by `rules`.
pub fn read(sql: &str, rules: &'static Rules) -> R<Read> {
    CX.with(|c| {
        *c.borrow_mut() = Some(Cx {
            rules,
            max_param: 0,
            names: vec![],
            notes: vec![],
        })
    });
    let out = (|| {
        let d = (rules.dialect)();
        // MySQL's default sql_mode makes `||` a logical OR with OR's precedence, but the parser
        // gives it concatenation's. Rewrite the operator token in the text (by position, so nothing
        // else is re-spelled) before parsing, and say so.
        let rewritten;
        let sql = if rules.name == "mysql" {
            match pipes_to_or(sql, d.as_ref()) {
                Some(s) => {
                    add_note(
                        "pipes_as_or",
                        "MySQL `||` is logical OR in the default sql_mode; under PIPES_AS_CONCAT it is concatenation",
                        Allowance::PipesAsLogicalOr,
                    );
                    rewritten = s;
                    rewritten.as_str()
                }
                None => sql,
            }
        } else {
            sql
        };
        let mut stmts =
            Parser::parse_sql(d.as_ref(), sql).map_err(|e| ReadError::Syntax(e.to_string()))?;
        if stmts.len() != 1 {
            return no(
                "multiple_statements",
                &format!("expected one statement, found {}", stmts.len()),
            );
        }
        statement(stmts.remove(0))
    })();
    let cx = CX.with(|c| c.borrow_mut().take()).expect("reader context");
    out.map(|stmt| Read {
        stmt,
        notes: cx.notes,
    })
}

/// `sql` with each `||` operator token replaced by ` OR `; `None` when there is none (or the text does
/// not tokenize, which the parser then reports).
fn pipes_to_or(sql: &str, dialect: &dyn sqlparser::dialect::Dialect) -> Option<String> {
    use sqlparser::tokenizer::{Token, Tokenizer};
    let tokens = Tokenizer::new(dialect, sql).tokenize_with_location().ok()?;
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(
            sql.chars()
                .enumerate()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i + 1),
        )
        .collect();
    let mut spots: Vec<usize> = tokens
        .iter()
        .filter(|t| t.token == Token::StringConcat)
        .filter_map(|t| {
            let line = usize::try_from(t.span.start.line).ok()?.checked_sub(1)?;
            let col = usize::try_from(t.span.start.column).ok()?.checked_sub(1)?;
            Some(line_starts.get(line)? + col)
        })
        .collect();
    if spots.is_empty() {
        return None;
    }
    spots.sort_unstable();
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    for s in spots {
        if s < at || s + 2 > chars.len() || chars[s] != '|' || chars[s + 1] != '|' {
            return None;
        }
        out.extend(&chars[at..s]);
        out.push_str(" OR ");
        at = s + 2;
    }
    out.extend(&chars[at..]);
    Some(out)
}

/// The classes of the base-table columns a DDL script declares, read for types only: this is not DDL
/// translation, and no statement of it is ever written out.
#[derive(Clone, Debug, Default)]
pub struct SchemaRead {
    pub schema: Schema,
    /// Statements the parser could not read (a function body, a `DO` block): each head and reason.
    /// They are skipped, which can only leave a column unregistered (unknown).
    pub skipped: Vec<String>,
}

/// Class of a declared type, from its spelling (`None`: a type UniSQL does not classify). MySQL's
/// `tinyint(1)` is numeric; `boolean` is `Boolean` only because the declared type says so.
fn class_of_type(dt: &sp::DataType) -> Option<ColumnClass> {
    let t = dt.to_string().to_ascii_lowercase();
    if t.ends_with("[]") || t.starts_with("array") {
        return None;
    }
    let base: String = t
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ' ')
        .collect();
    let base = base.trim();
    let first = base.split_whitespace().next().unwrap_or("");
    Some(match base {
        "character varying" | "character" | "national character varying" | "national character" => {
            ColumnClass::Text
        }
        "double precision" => ColumnClass::Numeric,
        b if b.starts_with("timestamp")
            || b == "time"
            || b.starts_with("time with")
            || b.starts_with("time without") =>
        {
            ColumnClass::Other
        }
        _ => match first {
            "text" | "varchar" | "char" | "nchar" | "nvarchar" | "tinytext" | "mediumtext"
            | "longtext" | "string" | "bpchar" | "clob" => ColumnClass::Text,
            "int" | "integer" | "smallint" | "bigint" | "tinyint" | "mediumint" | "int2"
            | "int4" | "int8" | "serial" | "bigserial" | "smallserial" | "decimal" | "numeric"
            | "dec" | "float" | "float4" | "float8" | "real" | "double" | "number" => {
                ColumnClass::Numeric
            }
            "boolean" | "bool" => ColumnClass::Boolean,
            "date" | "datetime" | "timestamptz" | "year" | "interval" | "uuid" | "json"
            | "jsonb" | "bytea" | "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary"
            | "varbinary" | "bit" | "inet" | "cidr" | "macaddr" => ColumnClass::Other,
            _ => return None,
        },
    })
}

fn column_from_def(c: &sp::ColumnDef) -> Column {
    let class = class_of_type(&c.data_type);
    let collation = c.options.iter().find_map(|o| match &o.option {
        sp::ColumnOption::Collation(n) => {
            Some(n.to_string().trim_matches(['"', '`', '\'']).to_string())
        }
        _ => None,
    });
    let mut col = Column::new(class.unwrap_or(ColumnClass::Unknown));
    col.collation = collation;
    col
}

/// Read `CREATE TABLE`, `ALTER TABLE .. ADD|DROP|ALTER COLUMN` and `DROP TABLE` out of a DDL script
/// (a migration file) in the dialect described by `rules`. Statements are split on `;` tokens, so a
/// dollar-quoted body does not split; a statement that does not parse is listed in `skipped`.
pub fn read_schema(ddl: &str, rules: &'static Rules) -> R<SchemaRead> {
    // `ident` consults the reader context for quoted-name folding, so one must be installed.
    CX.with(|c| {
        *c.borrow_mut() = Some(Cx {
            rules,
            max_param: 0,
            names: vec![],
            notes: vec![],
        })
    });
    let out = read_schema_inner(ddl, rules);
    CX.with(|c| c.borrow_mut().take());
    out
}

fn read_schema_inner(ddl: &str, rules: &'static Rules) -> R<SchemaRead> {
    use sqlparser::tokenizer::{Token, Tokenizer};
    let d = (rules.dialect)();
    let tokens = Tokenizer::new(d.as_ref(), ddl)
        .tokenize_with_location()
        .map_err(|e| ReadError::Syntax(e.to_string()))?;
    let chars: Vec<char> = ddl.chars().collect();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(
            chars
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == '\n')
                .map(|(i, _)| i + 1),
        )
        .collect();
    let offset = |loc: sqlparser::tokenizer::Location| -> Option<usize> {
        let line = usize::try_from(loc.line).ok()?.checked_sub(1)?;
        let col = usize::try_from(loc.column).ok()?.checked_sub(1)?;
        Some(line_starts.get(line)? + col)
    };
    let mut cuts: Vec<usize> = tokens
        .iter()
        .filter(|t| t.token == Token::SemiColon)
        .filter_map(|t| offset(t.span.start))
        .collect();
    cuts.push(chars.len());
    let mut out = SchemaRead::default();
    let mut from = 0;
    for cut in cuts {
        if cut < from || cut > chars.len() {
            continue;
        }
        let text: String = chars[from..cut].iter().collect();
        from = (cut + 1).min(chars.len());
        if text.trim().is_empty() {
            continue;
        }
        let stmts = match Parser::parse_sql(d.as_ref(), &text) {
            Ok(s) => s,
            Err(e) => {
                let head: String = text
                    .split_whitespace()
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(" ");
                out.skipped.push(format!("{head}: {e}"));
                continue;
            }
        };
        for st in stmts {
            apply_ddl(&mut out.schema, st);
        }
    }
    Ok(out)
}

fn apply_ddl(schema: &mut Schema, st: sp::Statement) {
    match st {
        sp::Statement::CreateTable(ct) => {
            if ct.query.is_some() || ct.like.is_some() {
                return;
            }
            let Ok(name) = object_name(&ct.name) else {
                return;
            };
            schema.drop_table(&name);
            schema.mark_columns_complete(name.0.clone());
            for c in &ct.columns {
                schema.set_column(name.0.clone(), ident(&c.name), column_from_def(c));
                for o in &c.options {
                    if matches!(
                        o.option,
                        sp::ColumnOption::PrimaryKey(_) | sp::ColumnOption::Unique(_)
                    ) {
                        schema.add_unique_key(name.0.clone(), vec![ident(&c.name)]);
                    }
                }
            }
            for con in &ct.constraints {
                add_constraint_key(schema, &name, con);
            }
        }
        sp::Statement::CreateIndex(ci) => {
            let Ok(name) = object_name(&ci.table_name) else {
                return;
            };
            if ci.unique {
                match index_columns(&ci.columns) {
                    Some(cols) if ci.predicate.is_none() => schema.add_unique_key(name.0, cols),
                    _ => schema.mark_keys_unreliable(name.0),
                }
            }
        }
        sp::Statement::AlterTable(at) => {
            let Ok(name) = object_name(&at.name) else {
                return;
            };
            for op in at.operations {
                match op {
                    sp::AlterTableOperation::AddColumn { column_def, .. } => {
                        schema.set_column(
                            name.0.clone(),
                            ident(&column_def.name),
                            column_from_def(&column_def),
                        );
                    }
                    sp::AlterTableOperation::AddConstraint { constraint, .. } => {
                        add_constraint_key(schema, &name, &constraint);
                    }
                    sp::AlterTableOperation::DropPrimaryKey { .. }
                    | sp::AlterTableOperation::DropConstraint { .. } => {
                        schema.mark_keys_unreliable(name.0.clone());
                    }
                    sp::AlterTableOperation::DropColumn { column_names, .. } => {
                        for c in &column_names {
                            schema.drop_column(&name, &ident(c));
                        }
                    }
                    sp::AlterTableOperation::AlterColumn {
                        column_name,
                        op: sp::AlterColumnOperation::SetDataType { data_type, .. },
                    } => {
                        let class = class_of_type(&data_type).unwrap_or(ColumnClass::Unknown);
                        // A new type resets the collation unless the statement states one.
                        schema.set_column(name.0.clone(), ident(&column_name), Column::new(class));
                    }
                    _ => {}
                }
            }
        }
        sp::Statement::Drop {
            object_type: sp::ObjectType::Index,
            ..
        } => schema.mark_all_keys_unreliable(),
        sp::Statement::Drop {
            object_type: sp::ObjectType::Table,
            names,
            ..
        } => {
            for n in &names {
                if let Ok(name) = object_name(n) {
                    schema.drop_table(&name);
                }
            }
        }
        _ => {}
    }
}

/// The column names of an index definition when every part is a plain column.
fn index_columns(cols: &[sp::IndexColumn]) -> Option<Vec<String>> {
    cols.iter()
        .map(|c| match &c.column.expr {
            sp::Expr::Identifier(i) => Some(ident(i)),
            _ => None,
        })
        .collect()
}

/// A PRIMARY KEY or UNIQUE table constraint: its columns are a unique key (an index over an
/// expression makes the table's key list unreliable).
fn add_constraint_key(schema: &mut Schema, table: &Name, con: &sp::TableConstraint) {
    let cols = match con {
        sp::TableConstraint::PrimaryKey(p) => &p.columns,
        sp::TableConstraint::Unique(u) => &u.columns,
        _ => return,
    };
    match index_columns(cols) {
        Some(c) => schema.add_unique_key(table.0.clone(), c),
        None => schema.mark_keys_unreliable(table.0.clone()),
    }
}

fn statement(s: sp::Statement) -> R<Statement> {
    match s {
        sp::Statement::Query(q)
            if matches!(
                &*q.body,
                sp::SetExpr::Insert(_) | sp::SetExpr::Update(_) | sp::SetExpr::Delete(_)
            ) =>
        {
            // `WITH .. INSERT|UPDATE|DELETE ..` is a query node in the parser's tree.
            let mut q = *q;
            let with = q.with.take();
            if q.to_string() != q.body.to_string() {
                return no(
                    "query_clause",
                    "clauses around a WITH .. INSERT/UPDATE/DELETE",
                );
            }
            let inner = match *q.body {
                sp::SetExpr::Insert(st) | sp::SetExpr::Update(st) | sp::SetExpr::Delete(st) => {
                    statement(st)?
                }
                _ => unreachable!(),
            };
            match with {
                None => Ok(inner),
                Some(w) => {
                    let (ctes, recursive) = read_ctes(w)?;
                    Ok(Statement::With(WithStatement {
                        recursive,
                        ctes,
                        stmt: Box::new(inner),
                    }))
                }
            }
        }
        sp::Statement::Query(q) => Ok(Statement::Query(Box::new(query(*q)?))),
        sp::Statement::Insert(i) => insert(i),
        sp::Statement::Update(u) => update(u),
        sp::Statement::Delete(d) => delete(d),
        other => no(
            "statement",
            &format!(
                "{} is not a DML statement the first slice reads",
                head(&other)
            ),
        ),
    }
}

fn head(x: &impl std::fmt::Debug) -> String {
    let s = format!("{x:?}");
    s.split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Identifier folding: unquoted identifiers fold to lower case; quoted ones are exact, except where
/// the dialect treats quoted names case-insensitively too (SQLite).
fn ident(i: &sp::Ident) -> String {
    if i.quote_style.is_some() && !rules().fold_quoted {
        i.value.clone()
    } else {
        i.value.to_lowercase()
    }
}

fn object_name(n: &sp::ObjectName) -> R<Name> {
    let mut parts = Vec::new();
    for p in &n.0 {
        match p {
            sp::ObjectNamePart::Identifier(i) => parts.push(ident(i)),
            other => return no("object_name_part", &head(other)),
        }
    }
    Ok(Name(parts))
}

fn single_name(n: &sp::ObjectName) -> R<String> {
    let nm = object_name(n)?;
    if nm.0.len() != 1 {
        return no(
            "qualified_column_target",
            "expected an unqualified column name",
        );
    }
    Ok(nm.0.into_iter().next().unwrap())
}

/// The `WITH` list of a query or of a data-modifying statement.
fn read_ctes(w: sp::With) -> R<(Vec<Cte>, bool)> {
    let sp::With {
        with_token: _,
        recursive,
        cte_tables,
    } = w;
    let mut ctes = Vec::new();
    for c in cte_tables {
        let sp::Cte {
            alias,
            query: cq,
            from,
            materialized,
            closing_paren_token: _,
        } = c;
        if from.is_some() || materialized.is_some() {
            return no("cte_option", "MATERIALIZED / FROM in a CTE");
        }
        let mut columns = Vec::new();
        for col in &alias.columns {
            if col.data_type.is_some() {
                return no("cte_column_type", "typed CTE column");
            }
            columns.push(ident(&col.name));
        }
        let dml = matches!(
            &*cq.body,
            sp::SetExpr::Insert(_) | sp::SetExpr::Update(_) | sp::SetExpr::Delete(_)
        );
        let cte_body = if dml {
            // Nothing but the statement itself: a WITH / ORDER BY / LIMIT around it would be
            // dropped silently otherwise.
            if cq.to_string() != cq.body.to_string() {
                return no("cte_dml_clause", "clauses around a data-modifying CTE");
            }
            match *cq.body {
                sp::SetExpr::Insert(st) | sp::SetExpr::Update(st) | sp::SetExpr::Delete(st) => {
                    CteBody::Dml(Box::new(statement(st)?))
                }
                _ => unreachable!(),
            }
        } else {
            CteBody::Query(Box::new(query(*cq)?))
        };
        ctes.push(Cte {
            name: ident(&alias.name),
            columns,
            body: cte_body,
        });
    }
    Ok((ctes, recursive))
}

fn query(q: sp::Query) -> R<Query> {
    let sp::Query {
        with,
        body,
        order_by,
        limit_clause,
        fetch,
        locks,
        for_clause,
        settings,
        format_clause,
        pipe_operators,
    } = q;
    if fetch.is_some() {
        return no("fetch_first", "FETCH FIRST is not in the IR (use LIMIT)");
    }
    if for_clause.is_some()
        || settings.is_some()
        || format_clause.is_some()
        || !pipe_operators.is_empty()
    {
        return no("query_clause", "FOR/SETTINGS/FORMAT/pipe operators");
    }
    let mut ctes = Vec::new();
    let mut recursive = false;
    if let Some(w) = with {
        (ctes, recursive) = read_ctes(w)?;
    }
    let mut body = set_expr(*body)?;
    let mut lock = None;
    match locks.len() {
        0 => {}
        1 => {
            let sp::LockClause {
                lock_type,
                of,
                nonblock,
            } = locks.into_iter().next().unwrap();
            lock = Some(RowLockClause {
                strength: match lock_type {
                    sp::LockType::Update => RowLock::Update,
                    sp::LockType::Share => RowLock::Share,
                },
                of: match of {
                    Some(n) => Some(object_name(&n)?),
                    None => None,
                },
                wait: match nonblock {
                    None => LockWait::Block,
                    Some(sp::NonBlock::Nowait) => LockWait::NoWait,
                    Some(sp::NonBlock::SkipLocked) => LockWait::SkipLocked,
                },
            });
        }
        _ => return no("row_lock_multiple", "several locking clauses"),
    }
    if let Some(l) = lock {
        match &mut body {
            SetExpr::Select(s) => s.lock = Some(l),
            _ => return no("row_lock_on_set_op", "locking clause on a set operation"),
        }
    }
    let mut order = Vec::new();
    if let Some(ob) = order_by {
        let sp::OrderBy { kind, interpolate } = ob;
        if interpolate.is_some() {
            return no("order_by_interpolate", "INTERPOLATE");
        }
        match kind {
            sp::OrderByKind::Expressions(items) => {
                for it in items {
                    order.push(order_item(it)?);
                }
            }
            sp::OrderByKind::All(_) => return no("order_by_all", "ORDER BY ALL"),
        }
    }
    let (mut limit, mut offset) = (None, None);
    match limit_clause {
        None => {}
        Some(sp::LimitClause::LimitOffset {
            limit: l,
            offset: o,
            limit_by,
        }) => {
            if !limit_by.is_empty() {
                return no("limit_by", "LIMIT .. BY");
            }
            limit = l.map(expr).transpose()?;
            if rules().negative_limit_unlimited {
                // SQLite: a negative LIMIT means "no limit".
                let neg = matches!(&limit, Some(Expr::Unary { op: UnaryOp::Minus, expr })
                    if matches!(**expr, Expr::Literal(Literal::Number(_))));
                if neg {
                    limit = None;
                }
            }
            if let Some(o) = o {
                let sp::Offset { value, rows } = o;
                if !matches!(rows, sp::OffsetRows::None) {
                    return no("offset_rows", "OFFSET .. ROWS");
                }
                offset = Some(expr(value)?);
            }
        }
        Some(sp::LimitClause::OffsetCommaLimit {
            offset: o,
            limit: l,
        }) => {
            offset = Some(expr(o)?);
            limit = Some(expr(l)?);
        }
    }
    Ok(Query {
        with: ctes,
        recursive,
        body,
        order_by: order,
        limit,
        offset,
    })
}

fn order_item(o: sp::OrderByExpr) -> R<OrderItem> {
    let sp::OrderByExpr {
        expr: e,
        options,
        with_fill,
    } = o;
    if with_fill.is_some() {
        return no("order_by_with_fill", "WITH FILL");
    }
    let desc = matches!(options.sort, Some(sp::OrderBySort::Desc));
    // The source's default NULL placement, made explicit so any target preserves it: NULL is the
    // largest value on PostgreSQL (first when descending) and the smallest on SQLite (first ascending).
    let default_first = if rules().nulls_smallest { !desc } else { desc };
    let nulls_first = options.nulls_first.unwrap_or(default_first);
    Ok(OrderItem {
        expr: expr(e)?,
        desc,
        nulls_first: Some(nulls_first),
    })
}

fn set_expr(s: sp::SetExpr) -> R<SetExpr> {
    match s {
        sp::SetExpr::Select(sel) => Ok(SetExpr::Select(Box::new(select(*sel)?))),
        sp::SetExpr::SetOperation {
            left,
            op,
            set_quantifier,
            right,
        } => {
            let op = match op {
                sp::SetOperator::Union => SetOp::Union,
                sp::SetOperator::Intersect => SetOp::Intersect,
                sp::SetOperator::Except => SetOp::Except,
                sp::SetOperator::Minus => return no("minus", "MINUS is not PostgreSQL"),
            };
            let all = match set_quantifier {
                sp::SetQuantifier::All => true,
                sp::SetQuantifier::Distinct | sp::SetQuantifier::None => false,
                other => return no("set_quantifier", &head(&other)),
            };
            Ok(SetExpr::SetOp {
                op,
                all,
                left: Box::new(set_expr(*left)?),
                right: Box::new(set_expr(*right)?),
            })
        }
        sp::SetExpr::Query(q) => {
            let q = query(*q)?;
            if !q.with.is_empty()
                || !q.order_by.is_empty()
                || q.limit.is_some()
                || q.offset.is_some()
            {
                return no(
                    "parenthesised_query",
                    "parenthesised query with WITH/ORDER/LIMIT as a set operand",
                );
            }
            Ok(q.body)
        }
        sp::SetExpr::Values(_) => no("values_query", "a bare VALUES query outside INSERT"),
        other => no("set_expr", &head(&other)),
    }
}

fn select(s: sp::Select) -> R<Select> {
    let sp::Select {
        select_token: _,
        optimizer_hints,
        distinct,
        select_modifiers,
        top,
        top_before_distinct: _,
        projection,
        exclude,
        into,
        from,
        lateral_views,
        prewhere,
        selection,
        connect_by,
        group_by,
        cluster_by,
        distribute_by,
        sort_by,
        having,
        named_window,
        qualify,
        window_before_qualify: _,
        value_table_mode,
        flavor: _,
    } = s;
    if !optimizer_hints.is_empty()
        || select_modifiers.is_some()
        || top.is_some()
        || exclude.is_some()
        || into.is_some()
        || !lateral_views.is_empty()
        || prewhere.is_some()
        || !connect_by.is_empty()
        || !cluster_by.is_empty()
        || !distribute_by.is_empty()
        || !sort_by.is_empty()
        || !named_window.is_empty()
        || qualify.is_some()
        || value_table_mode.is_some()
    {
        return no(
            "select_clause",
            "a SELECT clause the IR does not represent (TOP/INTO/WINDOW/QUALIFY/hints/...)",
        );
    }
    let distinct = match distinct {
        None | Some(sp::Distinct::All) => Distinct::No,
        Some(sp::Distinct::Distinct) => Distinct::Yes,
        Some(sp::Distinct::On(es)) => Distinct::On(es.into_iter().map(expr).collect::<R<_>>()?),
    };
    let group_by = match group_by {
        sp::GroupByExpr::Expressions(es, mods) => {
            if !mods.is_empty() {
                return no("group_by_modifier", "ROLLUP/CUBE/TOTALS modifiers");
            }
            es.into_iter().map(expr).collect::<R<Vec<_>>>()?
        }
        sp::GroupByExpr::All(_) => return no("group_by_all", "GROUP BY ALL"),
    };
    let mut proj = Vec::new();
    for it in projection {
        proj.push(select_item(it)?);
    }
    let mut from_refs = Vec::new();
    for t in from {
        from_refs.push(table_with_joins(t)?);
    }
    Ok(Select {
        distinct,
        projection: proj,
        from: from_refs,
        where_: selection.map(expr).transpose()?,
        group_by,
        having: having.map(expr).transpose()?,
        lock: None,
    })
}

fn wildcard_plain(o: &sp::WildcardAdditionalOptions) -> R<()> {
    let sp::WildcardAdditionalOptions {
        wildcard_token: _,
        opt_ilike,
        opt_exclude,
        opt_except,
        opt_replace,
        opt_rename,
        opt_alias,
    } = o;
    if opt_ilike.is_some()
        || opt_exclude.is_some()
        || opt_except.is_some()
        || opt_replace.is_some()
        || opt_rename.is_some()
        || opt_alias.is_some()
    {
        return no("wildcard_option", "EXCLUDE/EXCEPT/REPLACE/RENAME on *");
    }
    Ok(())
}

fn select_item(it: sp::SelectItem) -> R<SelectItem> {
    match it {
        sp::SelectItem::UnnamedExpr(e) => Ok(SelectItem::Expr {
            expr: expr(e)?,
            alias: None,
        }),
        sp::SelectItem::ExprWithAlias { expr: e, alias } => Ok(SelectItem::Expr {
            expr: expr(e)?,
            alias: Some(ident(&alias)),
        }),
        sp::SelectItem::Wildcard(o) => {
            wildcard_plain(&o)?;
            Ok(SelectItem::Wildcard)
        }
        sp::SelectItem::QualifiedWildcard(kind, o) => {
            wildcard_plain(&o)?;
            match kind {
                sp::SelectItemQualifiedWildcardKind::ObjectName(n) => {
                    Ok(SelectItem::QualifiedWildcard(object_name(&n)?))
                }
                other => no("qualified_wildcard", &head(&other)),
            }
        }
        other => no("select_item", &head(&other)),
    }
}

fn alias_name(a: Option<sp::TableAlias>) -> R<Option<String>> {
    match a {
        None => Ok(None),
        Some(sp::TableAlias {
            explicit: _,
            name,
            columns,
            at,
        }) => {
            if !columns.is_empty() || at.is_some() {
                return no("table_alias_columns", "column list on a table alias");
            }
            Ok(Some(ident(&name)))
        }
    }
}

fn table_factor(t: sp::TableFactor) -> R<TableRef> {
    match t {
        sp::TableFactor::Table {
            name,
            alias,
            args,
            with_hints,
            version,
            with_ordinality,
            partitions,
            json_path,
            sample,
            index_hints,
        } => {
            // A table-valued function: positional expression arguments only.
            if let Some(targs) = args {
                let plain = with_hints.is_empty()
                    && version.is_none()
                    && !with_ordinality
                    && partitions.is_empty()
                    && json_path.is_none()
                    && sample.is_none()
                    && index_hints.is_empty()
                    && targs.settings.is_none();
                let nm = object_name(&name)?;
                if plain && nm.0.len() == 1 {
                    let mut out = Vec::new();
                    for a in targs.args {
                        match a {
                            sp::FunctionArg::Unnamed(sp::FunctionArgExpr::Expr(e)) => {
                                out.push(expr(e)?)
                            }
                            _ => {
                                return no(
                                    "table_function_arg",
                                    "named or wildcard argument of a table function",
                                );
                            }
                        }
                    }
                    return Ok(TableRef::Function {
                        name: nm.0[0].clone(),
                        args: out,
                        alias: alias_name(alias)?,
                    });
                }
                return no(
                    "table_option",
                    "table function args/hints/version/sample/partitions",
                );
            }
            if !with_hints.is_empty()
                || version.is_some()
                || with_ordinality
                || !partitions.is_empty()
                || json_path.is_some()
                || sample.is_some()
                || !index_hints.is_empty()
            {
                return no(
                    "table_option",
                    "table function args/hints/version/sample/partitions",
                );
            }
            Ok(TableRef::Table {
                name: object_name(&name)?,
                alias: alias_name(alias)?,
            })
        }
        sp::TableFactor::Derived {
            lateral,
            subquery,
            alias,
            sample,
        } => {
            if lateral || sample.is_some() {
                return no("derived_option", "LATERAL / TABLESAMPLE");
            }
            let alias = alias_name(alias)?.ok_or_else(|| ReadError::NotInIr {
                construct: "unaliased_subquery".into(),
                detail: "PostgreSQL requires an alias on a FROM subquery".into(),
            })?;
            Ok(TableRef::Subquery {
                query: Box::new(query(*subquery)?),
                alias,
            })
        }
        other => no("table_factor", &head(&other)),
    }
}

fn table_with_joins(t: sp::TableWithJoins) -> R<TableRef> {
    let sp::TableWithJoins { relation, joins } = t;
    let mut cur = table_factor(relation)?;
    for j in joins {
        let sp::Join {
            relation,
            global,
            join_operator,
        } = j;
        if global {
            return no("global_join", "GLOBAL JOIN");
        }
        let (kind, constraint) = match join_operator {
            sp::JoinOperator::Join(c) | sp::JoinOperator::Inner(c) => (JoinKind::Inner, c),
            sp::JoinOperator::Left(c) | sp::JoinOperator::LeftOuter(c) => (JoinKind::Left, c),
            sp::JoinOperator::Right(c) | sp::JoinOperator::RightOuter(c) => (JoinKind::Right, c),
            sp::JoinOperator::FullOuter(c) => (JoinKind::Full, c),
            sp::JoinOperator::CrossJoin(c) => (JoinKind::Cross, c),
            other => return no("join_kind", &head(&other)),
        };
        let (on, using, natural) = match constraint {
            sp::JoinConstraint::On(e) => (Some(expr(e)?), Vec::new(), false),
            sp::JoinConstraint::None => (None, Vec::new(), false),
            sp::JoinConstraint::Using(cols) => (
                None,
                cols.iter().map(single_name).collect::<R<Vec<_>>>()?,
                false,
            ),
            sp::JoinConstraint::Natural => (None, Vec::new(), true),
        };
        if natural && kind == JoinKind::Cross {
            return no("natural_join", "NATURAL CROSS JOIN");
        }
        cur = TableRef::Join {
            left: Box::new(cur),
            right: Box::new(table_factor(relation)?),
            kind,
            on,
            using,
            natural,
        };
    }
    Ok(cur)
}

fn simple_table(t: sp::TableWithJoins) -> R<(Name, Option<String>)> {
    let sp::TableWithJoins { relation, joins } = t;
    if !joins.is_empty() {
        return no("dml_target_join", "joins in a DML target");
    }
    match table_factor(relation)? {
        TableRef::Table { name, alias } => Ok((name, alias)),
        _ => no("dml_target", "DML target is not a plain table"),
    }
}

fn returning(r: Option<Vec<sp::SelectItem>>) -> R<Vec<SelectItem>> {
    r.unwrap_or_default().into_iter().map(select_item).collect()
}

fn assignments(a: Vec<sp::Assignment>) -> R<Vec<Assignment>> {
    let mut out = Vec::new();
    for sp::Assignment { target, value } in a {
        let column = match target {
            sp::AssignmentTarget::ColumnName(n) => single_name(&n)?,
            sp::AssignmentTarget::Tuple(_) => return no("tuple_assignment", "SET (a, b) = ..."),
        };
        out.push(Assignment {
            column,
            value: expr(value)?,
        });
    }
    Ok(out)
}

fn insert(i: sp::Insert) -> R<Statement> {
    let sp::Insert {
        insert_token: _,
        optimizer_hints,
        or,
        ignore,
        into: _,
        table,
        table_alias,
        columns,
        overwrite,
        source,
        assignments: asg,
        partitioned,
        after_columns,
        has_table_keyword: _,
        on,
        returning: ret,
        output,
        replace_into,
        priority,
        insert_alias,
        settings,
        format_clause,
        multi_table_insert_type,
        multi_table_into_clauses,
        multi_table_when_clauses,
        multi_table_else_clause,
    } = i;
    if !optimizer_hints.is_empty()
        || or.is_some()
        || table_alias.is_some()
        || overwrite
        || (!asg.is_empty() && (!columns.is_empty() || source.is_some()))
        || partitioned.is_some()
        || !after_columns.is_empty()
        || output.is_some()
        || priority.is_some()
        || insert_alias.is_some()
        || settings.is_some()
        || format_clause.is_some()
        || multi_table_insert_type.is_some()
        || !multi_table_into_clauses.is_empty()
        || !multi_table_when_clauses.is_empty()
        || multi_table_else_clause.is_some()
    {
        return no(
            "insert_option",
            "an INSERT form other than INSERT INTO t [(cols)] VALUES|query",
        );
    }
    let table = match table {
        sp::TableObject::TableName(n) => object_name(&n)?,
        other => return no("insert_target", &head(&other)),
    };
    // MySQL `INSERT INTO t SET a = 1, b = 2` is `INSERT INTO t (a, b) VALUES (1, 2)`.
    let (columns, set_source) = if asg.is_empty() {
        (
            columns.iter().map(single_name).collect::<R<Vec<_>>>()?,
            None,
        )
    } else {
        let set = assignments(asg)?;
        let (cols, vals): (Vec<_>, Vec<_>) = set.into_iter().map(|a| (a.column, a.value)).unzip();
        (cols, Some(InsertSource::Values(vec![vals])))
    };
    let source = match source {
        None if set_source.is_some() => set_source.unwrap_or(InsertSource::DefaultValues),
        None => InsertSource::DefaultValues,
        Some(q) => {
            let q = *q;
            match *q.body {
                sp::SetExpr::Values(v)
                    if q.with.is_none()
                        && q.order_by.is_none()
                        && q.limit_clause.is_none()
                        && q.locks.is_empty() =>
                {
                    let sp::Values {
                        explicit_row,
                        value_keyword: _,
                        rows,
                    } = v;
                    if explicit_row {
                        return no("explicit_row", "ROW(..) in VALUES");
                    }
                    let mut out = Vec::new();
                    for r in rows {
                        out.push(r.content.into_iter().map(expr).collect::<R<Vec<_>>>()?);
                    }
                    InsertSource::Values(out)
                }
                body => {
                    let q2 = sp::Query {
                        body: Box::new(body),
                        ..q
                    };
                    InsertSource::Query(Box::new(query(q2)?))
                }
            }
        }
    };
    let on_conflict = match on {
        None => None,
        Some(sp::OnInsert::OnConflict(oc)) => {
            let sp::OnConflict {
                conflict_target,
                action,
            } = oc;
            let target = match conflict_target {
                None => Vec::new(),
                Some(sp::ConflictTarget::Columns(cols)) => cols.iter().map(ident).collect(),
                Some(sp::ConflictTarget::OnConstraint(_)) => {
                    return no("on_conflict_constraint", "ON CONFLICT ON CONSTRAINT");
                }
            };
            let action = match action {
                sp::OnConflictAction::DoNothing => ConflictAction::DoNothing,
                sp::OnConflictAction::DoUpdate(sp::DoUpdate {
                    assignments: a,
                    selection,
                }) => ConflictAction::DoUpdate {
                    set: assignments(a)?,
                    where_: selection.map(expr).transpose()?,
                },
            };
            Some(OnConflict { target, action })
        }
        Some(sp::OnInsert::DuplicateKeyUpdate(a)) => {
            // No conflict target: MySQL fires on ANY unique key. `VALUES(c)` is `excluded.c`.
            let set = assignments(a)?
                .into_iter()
                .map(|asg| Assignment {
                    column: asg.column,
                    value: excluded_refs(asg.value),
                })
                .collect();
            Some(OnConflict {
                target: vec![],
                action: ConflictAction::DoUpdate { set, where_: None },
            })
        }
        Some(other) => return no("on_insert", &head(&other)),
    };
    // MySQL `INSERT IGNORE`: a unique-key conflict skips the row, as `ON CONFLICT DO NOTHING`
    // (with no target) does. IGNORE also downgrades other errors, hence the allowance.
    let on_conflict = if ignore {
        if on_conflict.is_some() {
            return no(
                "insert_option",
                "IGNORE together with ON DUPLICATE KEY UPDATE",
            );
        }
        add_note(
            "insert_ignore",
            "INSERT IGNORE downgrades every error of the statement to a warning; ON CONFLICT DO NOTHING \
             skips unique-key conflicts only",
            Allowance::IgnoreCoversOnlyUniqueConflicts,
        );
        Some(OnConflict {
            target: vec![],
            action: ConflictAction::DoNothing,
        })
    } else {
        on_conflict
    };
    Ok(Statement::Insert(Insert {
        table,
        replace: replace_into,
        columns,
        source,
        on_conflict,
        returning: returning(ret)?,
    }))
}

fn update(u: sp::Update) -> R<Statement> {
    let sp::Update {
        update_token: _,
        optimizer_hints,
        table,
        assignments: asg,
        from,
        selection,
        returning: ret,
        output,
        or,
        order_by,
        limit,
    } = u;
    if !optimizer_hints.is_empty()
        || output.is_some()
        || or.is_some()
        || !order_by.is_empty()
        || limit.is_some()
    {
        return no(
            "update_option",
            "UPDATE with hints/OUTPUT/OR/ORDER BY/LIMIT",
        );
    }
    let (table, alias) = simple_table(table)?;
    // The parser does not know MySQL's `UPDATE LOW_PRIORITY|IGNORE t ...` and reads the modifier as
    // the table name and `t` as its alias: refuse rather than write another table's name.
    if alias.is_some()
        && table.0.len() == 1
        && matches!(table.0[0].as_str(), "low_priority" | "ignore")
    {
        return no("update_option", "UPDATE LOW_PRIORITY / IGNORE");
    }
    let from = match from {
        None => Vec::new(),
        Some(sp::UpdateTableFromKind::AfterSet(ts)) => ts
            .into_iter()
            .map(table_with_joins)
            .collect::<R<Vec<_>>>()?,
        Some(other) => return no("update_from_position", &head(&other)),
    };
    Ok(Statement::Update(Update {
        table,
        alias,
        set: assignments(asg)?,
        from,
        where_: selection.map(expr).transpose()?,
        returning: returning(ret)?,
    }))
}

fn delete(d: sp::Delete) -> R<Statement> {
    let sp::Delete {
        delete_token: _,
        optimizer_hints,
        tables,
        from,
        using,
        selection,
        returning: ret,
        output,
        order_by,
        limit,
    } = d;
    if !optimizer_hints.is_empty()
        || !tables.is_empty()
        || output.is_some()
        || !order_by.is_empty()
        || limit.is_some()
    {
        return no(
            "delete_option",
            "DELETE with hints/multi-table/OUTPUT/ORDER BY/LIMIT",
        );
    }
    let ts = match from {
        sp::FromTable::WithFromKeyword(ts) | sp::FromTable::WithoutKeyword(ts) => ts,
    };
    if ts.len() != 1 {
        return no("delete_multi_target", "DELETE FROM a, b");
    }
    let (table, alias) = simple_table(ts.into_iter().next().unwrap())?;
    let using = using
        .unwrap_or_default()
        .into_iter()
        .map(table_with_joins)
        .collect::<R<Vec<_>>>()?;
    Ok(Statement::Delete(Delete {
        table,
        alias,
        using,
        where_: selection.map(expr).transpose()?,
        returning: returning(ret)?,
    }))
}

fn boxed(e: sp::Expr) -> R<Box<Expr>> {
    Ok(Box::new(expr(e)?))
}

fn ceil_floor(name: &str, x: sp::Expr, field: sp::CeilFloorKind) -> R<Expr> {
    if !matches!(
        field,
        sp::CeilFloorKind::DateTimeField(sp::DateTimeField::NoDateTime)
    ) {
        return no("ceil_form", "CEIL/FLOOR(x TO unit) and the scale form");
    }
    Ok(Expr::Function(Function {
        name: name.to_string(),
        args: vec![FunctionArg::Expr(expr(x)?)],
        distinct: false,
        star: false,
        filter: None,
        over: None,
        order_by: vec![],
    }))
}

fn is_bool(x: sp::Expr, value: bool, negated: bool) -> R<Expr> {
    Ok(Expr::IsBool {
        expr: boxed(x)?,
        value,
        negated,
    })
}

fn expr(e: sp::Expr) -> R<Expr> {
    use sp::Expr as E;
    Ok(match e {
        // `DEFAULT` in VALUES / SET is parsed as an identifier; as a column it would be written
        // quoted and mean something else (PostgreSQL rejects it, MySQL would read a column).
        E::Identifier(i) if i.quote_style.is_none() && i.value.eq_ignore_ascii_case("default") => {
            return no("default_value", "DEFAULT as a value is not in the IR");
        }
        E::Identifier(i) => Expr::Column(Name::simple(ident(&i))),
        E::CompoundIdentifier(parts) => Expr::Column(Name(parts.iter().map(ident).collect())),
        E::Value(v) => value(v.value)?,
        E::Nested(inner) => Expr::Nested(boxed(*inner)?),
        E::IsNull(x) => Expr::IsNull {
            expr: boxed(*x)?,
            negated: false,
        },
        E::IsNotNull(x) => Expr::IsNull {
            expr: boxed(*x)?,
            negated: true,
        },
        E::IsTrue(x) => is_bool(*x, true, false)?,
        E::IsNotTrue(x) => is_bool(*x, true, true)?,
        E::IsFalse(x) => is_bool(*x, false, false)?,
        E::IsNotFalse(x) => is_bool(*x, false, true)?,
        E::InList {
            expr: x,
            list,
            negated,
        } => Expr::InList {
            expr: boxed(*x)?,
            list: list.into_iter().map(expr).collect::<R<_>>()?,
            negated,
        },
        E::InSubquery {
            expr: x,
            subquery,
            negated,
        } => Expr::InSubquery {
            expr: boxed(*x)?,
            query: Box::new(query(*subquery)?),
            negated,
        },
        E::Between {
            expr: x,
            negated,
            low,
            high,
        } => Expr::Between {
            expr: boxed(*x)?,
            low: boxed(*low)?,
            high: boxed(*high)?,
            negated,
        },
        E::Like {
            negated,
            any,
            expr: x,
            pattern,
            escape_char,
        } => {
            if any {
                return no("like_option", "LIKE ANY");
            }
            let ci = rules().like_ci_default;
            if ci {
                add_note(
                    "like",
                    "the source LIKE is ASCII case-insensitive only while its connection setting is at the default",
                    Allowance::LikeCaseSensitivityIsConnectionSetting,
                );
            }
            if rules().name == "mysql" {
                add_note(
                    "like",
                    "the source LIKE follows the column collation; read as case-sensitive (a binary collation)",
                    Allowance::LikeCaseSensitivityIsConnectionSetting,
                );
            }
            like_expr(*x, *pattern, escape_char, negated, ci)?
        }
        E::RLike {
            negated,
            expr: x,
            pattern,
            ..
        } => Expr::Regexp {
            expr: boxed(*x)?,
            pattern: boxed(*pattern)?,
            negated,
        },
        E::ILike {
            negated,
            any,
            expr: x,
            pattern,
            escape_char,
        } => {
            if any {
                return no("like_option", "ILIKE ANY");
            }
            like_expr(*x, *pattern, escape_char, negated, true)?
        }
        E::UnaryOp { op, expr: x } => {
            let op = match op {
                sp::UnaryOperator::Not => UnaryOp::Not,
                sp::UnaryOperator::Minus => UnaryOp::Minus,
                sp::UnaryOperator::Plus => UnaryOp::Plus,
                other => return no("unary_operator", &head(&other)),
            };
            Expr::Unary {
                op,
                expr: boxed(*x)?,
            }
        }
        E::BinaryOp { left, op, right }
            if matches!(
                op,
                sp::BinaryOperator::PGRegexMatch | sp::BinaryOperator::PGRegexNotMatch
            ) =>
        {
            Expr::Regexp {
                expr: boxed(*left)?,
                pattern: boxed(*right)?,
                negated: matches!(op, sp::BinaryOperator::PGRegexNotMatch),
            }
        }
        E::BinaryOp { left, op, right } => {
            use sp::BinaryOperator as B;
            let op = match op {
                B::Eq => BinaryOp::Eq,
                B::NotEq => BinaryOp::NotEq,
                B::Lt => BinaryOp::Lt,
                B::LtEq => BinaryOp::LtEq,
                B::Gt => BinaryOp::Gt,
                B::GtEq => BinaryOp::GtEq,
                B::And => BinaryOp::And,
                B::Or => BinaryOp::Or,
                B::Plus => BinaryOp::Plus,
                B::Minus => BinaryOp::Minus,
                B::Multiply => BinaryOp::Multiply,
                B::Divide => BinaryOp::Divide,
                B::Modulo => BinaryOp::Modulo,
                // MySQL's default sql_mode makes `||` a logical OR with OR's (lowest) precedence, but
                // sqlparser parses it as concatenation with concatenation's precedence, so the tree
                // is wrong either way: refuse instead of guessing.
                B::StringConcat if rules().name == "mysql" => {
                    return no(
                        "pipes_operator",
                        "MySQL `||` is logical OR (default sql_mode) but the parser gives it concatenation precedence; write OR, or CONCAT()",
                    );
                }
                B::StringConcat => BinaryOp::Concat,
                B::MyIntegerDivide => BinaryOp::IntDivide,
                B::Spaceship => BinaryOp::NullSafeEq,
                B::BitwiseAnd => BinaryOp::BitAnd,
                B::BitwiseOr => BinaryOp::BitOr,
                B::BitwiseXor | B::PGBitwiseXor => BinaryOp::BitXor,
                B::PGBitwiseShiftLeft => BinaryOp::ShiftLeft,
                B::PGBitwiseShiftRight => BinaryOp::ShiftRight,
                B::Xor => BinaryOp::Xor,
                B::Arrow => BinaryOp::JsonGet,
                B::LongArrow => BinaryOp::JsonGetText,
                other => {
                    return no(
                        "binary_operator",
                        &format!(
                            "{} (JSON/array/regex/bitwise operators are not in the IR)",
                            head(&other)
                        ),
                    );
                }
            };
            if op == BinaryOp::Divide && rules().name == "mysql" {
                add_note(
                    "division",
                    "the source `/` is exact (DECIMAL) division; the target divides as double precision",
                    Allowance::DecimalDivisionAsFloat,
                );
                let l = Expr::Cast {
                    expr: boxed(*left)?,
                    ty: SqlType::Double,
                };
                return Ok(Expr::Binary {
                    op,
                    left: Box::new(l),
                    right: boxed(*right)?,
                });
            }
            // `ts +/- INTERVAL 'n unit'` is the same statement as `ts +/- make_interval(unit => n)`:
            // one IR form, so every target's interval arithmetic (and its refusal of months/years)
            // applies unchanged. `INTERVAL 'n unit' + ts` commutes for `+` only.
            match (op, *left, *right) {
                (BinaryOp::Plus | BinaryOp::Minus, l, E::Interval(iv)) => Expr::Binary {
                    op,
                    left: boxed(l)?,
                    right: Box::new(interval_call(iv)?),
                },
                (BinaryOp::Plus, E::Interval(iv), r) => Expr::Binary {
                    op,
                    left: boxed(r)?,
                    right: Box::new(interval_call(iv)?),
                },
                (op, l, r) => Expr::Binary {
                    op,
                    left: boxed(l)?,
                    right: boxed(r)?,
                },
            }
        }
        E::Case {
            case_token: _,
            end_token: _,
            operand,
            conditions,
            else_result,
        } => Expr::Case {
            operand: operand.map(|o| boxed(*o)).transpose()?,
            branches: conditions
                .into_iter()
                .map(|sp::CaseWhen { condition, result }| Ok((expr(condition)?, expr(result)?)))
                .collect::<R<_>>()?,
            else_: else_result.map(|o| boxed(*o)).transpose()?,
        },
        E::Cast {
            kind,
            expr: x,
            data_type,
            format,
        } => {
            if format.is_some() || !matches!(kind, sp::CastKind::Cast | sp::CastKind::DoubleColon) {
                return no("cast_form", "TRY_CAST / SAFE_CAST / CAST .. FORMAT");
            }
            Expr::Cast {
                expr: boxed(*x)?,
                ty: data_type_to_ir(&data_type)?,
            }
        }
        // MySQL `CONVERT(expr, type)` is `CAST(expr AS type)`; `CONVERT(expr USING cs)` converts the
        // character set, which is not a cast.
        E::Convert {
            is_try: false,
            expr: x,
            data_type: Some(data_type),
            charset: None,
            target_before_value: false,
            styles,
        } if styles.is_empty() => Expr::Cast {
            expr: boxed(*x)?,
            ty: data_type_to_ir(&data_type)?,
        },
        E::Convert { .. } => {
            return no(
                "convert_form",
                "CONVERT(.. USING charset), TRY_CONVERT and the target-first (MSSQL) form",
            );
        }
        // `CEIL(x)` / `FLOOR(x)` of a number. `CEIL(x TO unit)` and the scale form are not mapped.
        E::Ceil { expr: x, field } => ceil_floor("ceil", *x, field)?,
        E::Floor { expr: x, field } => ceil_floor("floor", *x, field)?,
        // `POSITION(sub IN str)` is `strpos(str, sub)`.
        E::Position { expr: sub, r#in } => Expr::Function(Function {
            name: "strpos".to_string(),
            args: vec![
                FunctionArg::Expr(expr(*r#in)?),
                FunctionArg::Expr(expr(*sub)?),
            ],
            distinct: false,
            star: false,
            filter: None,
            over: None,
            order_by: vec![],
        }),
        E::Trim {
            trim_where,
            trim_what,
            expr: x,
            trim_characters,
        } => {
            // `trim(chars FROM x)` / `trim(BOTH chars FROM x)`; the comma form is `trim_characters`.
            let side = match trim_where {
                None | Some(sp::TrimWhereField::Both) => TrimSide::Both,
                Some(sp::TrimWhereField::Leading) => TrimSide::Leading,
                Some(sp::TrimWhereField::Trailing) => TrimSide::Trailing,
            };
            let chars = match (trim_what, trim_characters) {
                (Some(w), None) => Some(boxed(*w)?),
                (None, None) => None,
                (None, Some(mut cs)) if cs.len() == 1 => Some(boxed(cs.remove(0))?),
                _ => return no("trim_form", "TRIM with several character arguments"),
            };
            Expr::Trim {
                expr: boxed(*x)?,
                side,
                chars,
            }
        }
        E::Substring {
            expr: x,
            substring_from,
            substring_for,
            special: _,
            shorthand: _,
        } => {
            if rules().name == "mysql" {
                // From position 1 and a non-negative length the engines agree; any other start
                // (0, negative, an expression) or length is the caller's stated assumption.
                let positive = |e: &E, min: u64| {
                    matches!(
                        e,
                        E::Value(v) if matches!(&v.value, sp::Value::Number(n, _)
                            if n.parse::<u64>().is_ok_and(|x| x >= min))
                    )
                };
                let start_ok = substring_from.as_deref().is_some_and(|f| positive(f, 1));
                let len_ok = substring_for.as_deref().is_none_or(|l| positive(l, 0));
                if !(start_ok && len_ok) {
                    add_note(
                        "fn:substr.start_position",
                        "MySQL SUBSTRING counts a start of 0 as empty and a negative start from the end; \
                         the target does not",
                        Allowance::SubstringStartAssumedPositive,
                    );
                }
            }
            // `substring(x FROM a FOR b)` and `substring(x, a, b)` are `substr(x, a, b)`.
            let mut args = vec![FunctionArg::Expr(expr(*x)?)];
            match (substring_from, substring_for) {
                (Some(f), Some(l)) => {
                    args.push(FunctionArg::Expr(expr(*f)?));
                    args.push(FunctionArg::Expr(expr(*l)?));
                }
                (Some(f), None) => args.push(FunctionArg::Expr(expr(*f)?)),
                (None, Some(l)) => {
                    // FOR without FROM starts at position 1.
                    args.push(FunctionArg::Expr(Expr::Literal(Literal::Number(
                        "1".into(),
                    ))));
                    args.push(FunctionArg::Expr(expr(*l)?));
                }
                (None, None) => return no("substring_form", "SUBSTRING without FROM or FOR"),
            }
            Expr::Function(Function {
                name: "substr".into(),
                args,
                distinct: false,
                star: false,
                filter: None,
                over: None,
                order_by: vec![],
            })
        }
        E::AnyOp {
            left,
            compare_op,
            right,
            is_some: _,
        } => {
            if !matches!(compare_op, sp::BinaryOperator::Eq) {
                return no("any_operator", "only `= ANY(..)` is in the IR");
            }
            quantified(*left, *right, false)?
        }
        E::AllOp {
            left,
            compare_op,
            right,
        } => {
            if !matches!(compare_op, sp::BinaryOperator::NotEq) {
                return no("all_operator", "only `<> ALL(..)` is in the IR");
            }
            quantified(*left, *right, true)?
        }
        E::Exists { subquery, negated } => Expr::Exists {
            query: Box::new(query(*subquery)?),
            negated,
        },
        E::Subquery(q) => Expr::Subquery(Box::new(query(*q)?)),
        E::Extract {
            field,
            syntax: _,
            expr: x,
        } => Expr::Extract {
            field: field.to_string().to_ascii_lowercase(),
            expr: boxed(*x)?,
        },
        E::Array(arr) => Expr::ArrayLiteral(arr.elem.into_iter().map(expr).collect::<R<Vec<_>>>()?),
        E::AtTimeZone {
            timestamp,
            time_zone,
        } => Expr::AtTimeZone {
            expr: boxed(*timestamp)?,
            zone: boxed(*time_zone)?,
        },
        E::Function(f) => function(f)?,
        other => return no("expression", &head(&other)),
    })
}

/// `x = ANY(rhs)` / `x <> ALL(rhs)`: an array parameter, an array literal, or a subquery.
fn quantified(left: sp::Expr, right: sp::Expr, negated: bool) -> R<Expr> {
    let lhs = boxed(left)?;
    // `$1::int[]` is only a type hint on the parameter.
    let mut right = right;
    if let sp::Expr::Cast {
        kind: sp::CastKind::Cast | sp::CastKind::DoubleColon,
        expr: inner,
        data_type: sp::DataType::Array(_),
        format: None,
    } = right.clone()
    {
        right = *inner;
    }
    match right {
        sp::Expr::Value(v) => match v.value {
            sp::Value::Placeholder(_) => match value(v.value)? {
                Expr::Param(n) => Ok(Expr::AnyParam {
                    expr: lhs,
                    param: n,
                    negated,
                }),
                _ => no("array_param", "expected a numbered parameter"),
            },
            other => no("array_operand", &head(&other)),
        },
        sp::Expr::Array(sp::Array { elem, named: _ }) => Ok(Expr::InList {
            expr: lhs,
            list: elem.into_iter().map(expr).collect::<R<Vec<_>>>()?,
            negated,
        }),
        sp::Expr::Subquery(q) => Ok(Expr::InSubquery {
            expr: lhs,
            query: Box::new(query(*q)?),
            negated,
        }),
        other => no("array_operand", &head(&other)),
    }
}

/// MySQL's `VALUES(col)` inside `ON DUPLICATE KEY UPDATE` refers to the row proposed for insertion.
fn excluded_refs(e: Expr) -> Expr {
    match e {
        Expr::Function(f) if f.name == "values" && f.args.len() == 1 => {
            match f.args.into_iter().next() {
                Some(FunctionArg::Expr(Expr::Column(n))) => {
                    Expr::Column(Name(vec!["excluded".to_string(), n.last().to_string()]))
                }
                other => Expr::Function(Function {
                    name: "values".into(),
                    args: other.into_iter().collect(),
                    distinct: false,
                    star: false,
                    filter: None,
                    over: None,
                    order_by: vec![],
                }),
            }
        }
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(excluded_refs(*left)),
            right: Box::new(excluded_refs(*right)),
        },
        Expr::Nested(x) => Expr::Nested(Box::new(excluded_refs(*x))),
        Expr::Unary { op, expr } => Expr::Unary {
            op,
            expr: Box::new(excluded_refs(*expr)),
        },
        other => other,
    }
}

fn like_expr(
    x: sp::Expr,
    pattern: sp::Expr,
    escape_char: Option<Box<sp::Expr>>,
    negated: bool,
    case_insensitive: bool,
) -> R<Expr> {
    let escape = match escape_char {
        Some(e) => Some(boxed(*e)?),
        None if rules().like_default_escape => {
            Some(Box::new(Expr::Literal(Literal::String("\\".into()))))
        }
        None => None,
    };
    Ok(Expr::Like {
        expr: boxed(x)?,
        pattern: boxed(pattern)?,
        negated,
        case_insensitive,
        escape,
    })
}

fn value(v: sp::Value) -> R<Expr> {
    Ok(match v {
        sp::Value::Number(n, long) => {
            if long {
                return no("numeric_suffix", "L suffix");
            }
            Expr::Literal(Literal::Number(n))
        }
        sp::Value::SingleQuotedString(s) => Expr::Literal(Literal::String(s)),
        // The tokenizer has already resolved the backslash escapes of `E'..'`.
        sp::Value::EscapedStringLiteral(s) => Expr::Literal(Literal::String(s)),
        sp::Value::Boolean(b) if rules().name == "mysql" => {
            // MySQL TRUE/FALSE are the integers 1 and 0.
            Expr::Literal(Literal::Number(if b { "1" } else { "0" }.into()))
        }
        sp::Value::Boolean(b) => Expr::Literal(Literal::Bool(b)),
        sp::Value::Null => Expr::Literal(Literal::Null),
        sp::Value::Placeholder(p) => {
            let bad = || ReadError::NotInIr {
                construct: "placeholder".into(),
                detail: format!(
                    "`{p}` is not a placeholder form of dialect `{}`",
                    rules().name
                ),
            };
            if rules().question_placeholders {
                if p == "?" {
                    Expr::Param(bump_param(None, None))
                } else if let Some(d) = p.strip_prefix('?') {
                    let n = d.parse::<u32>().ok().filter(|n| *n >= 1).ok_or_else(bad)?;
                    Expr::Param(bump_param(Some(n), None))
                } else if let Some(nm) = p.strip_prefix([':', '@', '$']) {
                    Expr::Param(bump_param(None, Some(nm)))
                } else {
                    return Err(bad());
                }
            } else {
                let n = p
                    .strip_prefix('$')
                    .and_then(|d| d.parse::<u32>().ok())
                    .filter(|n| *n >= 1)
                    .ok_or_else(bad)?;
                Expr::Param(n)
            }
        }
        other => return no("literal", &head(&other)),
    })
}

fn function(f: sp::Function) -> R<Expr> {
    let sp::Function {
        name,
        uses_odbc_syntax,
        parameters,
        args,
        within_group,
        filter,
        null_treatment,
        over,
    } = f;
    if uses_odbc_syntax
        || !matches!(parameters, sp::FunctionArguments::None)
        || !within_group.is_empty()
        || null_treatment.is_some()
    {
        return no(
            "function_option",
            "ODBC syntax/parameters/WITHIN GROUP/FILTER/IGNORE NULLS",
        );
    }
    let over = match over {
        None => None,
        Some(sp::WindowType::NamedWindow(_)) => {
            return no(
                "named_window",
                "OVER w (a WINDOW clause) is not in the IR yet",
            );
        }
        Some(sp::WindowType::WindowSpec(spec)) => Some(window_spec(spec)?),
    };
    let filter = filter.map(|f| expr(*f).map(Box::new)).transpose()?;
    let name = object_name(&name)?;
    let base = name.last().to_string();
    if name.0.len() != 1 {
        return no("qualified_function", &name.0.join("."));
    }
    let args = match args {
        // `CURRENT_TIMESTAMP` is a function without parentheses.
        sp::FunctionArguments::None => {
            if base == "current_timestamp" {
                if rules().name == "sqlite" {
                    add_note(
                        "fn:now",
                        "the source CURRENT_TIMESTAMP is text 'YYYY-MM-DD HH:MM:SS' (UTC), not a timestamptz",
                        Allowance::NowAsText,
                    );
                }
                return Ok(Expr::Function(Function {
                    name: "now".to_string(),
                    args: vec![],
                    distinct: false,
                    star: false,
                    filter: None,
                    over: None,
                    order_by: vec![],
                }));
            }
            // The session identity keywords are zero-argument functions.
            if matches!(
                base.as_str(),
                "current_user" | "session_user" | "current_role"
            ) {
                return Ok(Expr::Function(Function {
                    name: base,
                    args: vec![],
                    distinct: false,
                    star: false,
                    filter: None,
                    over: None,
                    order_by: vec![],
                }));
            }
            // `CURRENT_DATE`: a date in the session time zone (PostgreSQL, MySQL), text of the UTC
            // date on SQLite.
            if base == "current_date" {
                if rules().name == "sqlite" {
                    add_note(
                        "fn:current_date",
                        "the source CURRENT_DATE is text 'YYYY-MM-DD' (the UTC date), not a date",
                        Allowance::DateAsText,
                    );
                }
                return Ok(Expr::Function(Function {
                    name: "current_date".to_string(),
                    args: vec![],
                    distinct: false,
                    star: false,
                    filter: None,
                    over: None,
                    order_by: vec![],
                }));
            }
            return no("bare_function", &format!("`{base}` without parentheses"));
        }
        sp::FunctionArguments::Subquery(q) if base == "array" => {
            return Ok(Expr::ArraySubquery(Box::new(query(*q)?)));
        }
        sp::FunctionArguments::Subquery(_) => {
            return no("function_subquery", "function over a subquery");
        }
        sp::FunctionArguments::List(l) => l,
    };
    let sp::FunctionArgumentList {
        duplicate_treatment,
        args,
        clauses,
    } = args;
    let mut agg_order = Vec::new();
    let mut separator: Option<String> = None;
    for cl in clauses {
        match cl {
            sp::FunctionArgumentClause::OrderBy(items) => {
                for it in items {
                    agg_order.push(order_item(it)?);
                }
            }
            sp::FunctionArgumentClause::Separator(v) => match v.value {
                sp::Value::SingleQuotedString(s) => separator = Some(s),
                other => return no("separator", &head(&other)),
            },
            other => {
                return no(
                    "function_clause",
                    &format!("{} inside a function call", head(&other)),
                );
            }
        }
    }
    let distinct = match duplicate_treatment {
        None | Some(sp::DuplicateTreatment::All) => false,
        Some(sp::DuplicateTreatment::Distinct) => true,
    };
    let mut out = Vec::new();
    let mut star = false;
    for a in args {
        match a {
            sp::FunctionArg::Unnamed(sp::FunctionArgExpr::Expr(e)) => {
                out.push(FunctionArg::Expr(expr(e)?))
            }
            sp::FunctionArg::Unnamed(sp::FunctionArgExpr::Wildcard) => star = true,
            sp::FunctionArg::Named {
                name,
                arg: sp::FunctionArgExpr::Expr(e),
                operator: sp::FunctionArgOperator::RightArrow,
            } => out.push(FunctionArg::Named {
                name: ident(&name),
                value: expr(e)?,
            }),
            // sqlparser produces `ExprNamed` when the name is parsed as an expression.
            sp::FunctionArg::ExprNamed {
                name: sp::Expr::Identifier(name),
                arg: sp::FunctionArgExpr::Expr(e),
                operator: sp::FunctionArgOperator::RightArrow,
            } => out.push(FunctionArg::Named {
                name: ident(&name),
                value: expr(e)?,
            }),
            other => return no("function_argument", &head(&other)),
        }
    }
    let mut name = base;
    if name == "current_timestamp" {
        name = "now".to_string();
    }
    // CEILING is CEIL; MOD(a, b) is `a % b`.
    if name == "ceiling" {
        name = "ceil".to_string();
    }
    if name == "mod" && out.len() == 2 {
        let mut it = out.into_iter().map(|a| match a {
            FunctionArg::Expr(e) => e,
            FunctionArg::Named { value, .. } => value,
        });
        let (l, r) = (it.next().unwrap(), it.next().unwrap());
        return Ok(Expr::Binary {
            op: BinaryOp::Modulo,
            left: Box::new(l),
            right: Box::new(r),
        });
    }
    if rules().name == "mysql" {
        match name.as_str() {
            "ifnull" => name = "coalesce".to_string(),
            "curdate" if out.is_empty() => name = "current_date".to_string(),
            "group_concat" => {
                name = "string_agg".to_string();
                out.push(FunctionArg::Expr(Expr::Literal(Literal::String(
                    separator.clone().unwrap_or_else(|| ",".to_string()),
                ))));
                add_note(
                    "fn:string_agg",
                    "the source GROUP_CONCAT truncates silently at group_concat_max_len",
                    Allowance::AggregateStringTruncation,
                );
            }
            // MySQL LENGTH() counts bytes.
            "length" => name = "octet_length".to_string(),
            // `LOCATE(sub, str)` and `INSTR(str, sub)` are `strpos(str, sub)`: 1-based, 0 when absent,
            // NULL when either is NULL. The three-argument LOCATE (start position) is not mapped.
            "locate" | "instr" if out.len() == 2 => {
                if name == "locate" {
                    out.swap(0, 1);
                }
                name = "strpos".to_string();
                add_note(
                    "fn:strpos",
                    "the source LOCATE/INSTR compare by the column collation (case-insensitive unless binary)",
                    Allowance::TextCollation,
                );
            }
            "concat" if out.len() >= 2 => {
                let mut it = out.into_iter().map(|a| match a {
                    FunctionArg::Expr(e) => e,
                    FunctionArg::Named { value, .. } => value,
                });
                let first = it.next().expect("two arguments");
                let chain = it.fold(first, |l, r| Expr::Binary {
                    op: BinaryOp::Concat,
                    left: Box::new(l),
                    right: Box::new(r),
                });
                return Ok(chain);
            }
            "if" if out.len() == 3 => {
                let mut it = out.into_iter().map(|a| match a {
                    FunctionArg::Expr(e) => e,
                    FunctionArg::Named { value, .. } => value,
                });
                let (c, t, f) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap());
                return Ok(Expr::Case {
                    operand: None,
                    branches: vec![(c, t)],
                    else_: Some(Box::new(f)),
                });
            }
            _ => {}
        }
    }
    if rules().name == "sqlite" {
        match name.as_str() {
            "ifnull" => name = "coalesce".to_string(),
            "instr" if out.len() == 2 => name = "strpos".to_string(),
            "group_concat" => {
                name = "string_agg".to_string();
                if out.len() == 1 {
                    out.push(FunctionArg::Expr(Expr::Literal(Literal::String(
                        ",".into(),
                    ))));
                }
            }
            _ => {}
        }
    }
    Ok(Expr::Function(Function {
        name,
        args: out,
        distinct,
        star,
        filter,
        over,
        order_by: agg_order,
    }))
}

/// `INTERVAL '3 days'` / `INTERVAL 3 DAY` as `make_interval(days => 3)`. Only a single `<number> <unit>`
/// is represented; compound intervals (`'1 day 2 hours'`), ranges (`DAY TO SECOND`) and unit-less
/// strings are not in the IR. Months and years are read (and then refused by the targets, whose
/// month arithmetic differs), not guessed.
fn interval_call(iv: sp::Interval) -> R<Expr> {
    let sp::Interval {
        value,
        leading_field,
        leading_precision,
        last_field,
        fractional_seconds_precision,
    } = iv;
    if leading_precision.is_some() || last_field.is_some() || fractional_seconds_precision.is_some()
    {
        return no(
            "interval",
            "interval ranges and precisions are not in the IR",
        );
    }
    let (amount, unit) = match (*value, leading_field) {
        // MySQL `INTERVAL 3 DAY`
        (sp::Expr::Value(v), Some(field)) => (
            match v.value {
                sp::Value::Number(n, _) => n,
                other => return no("interval", &head(&other)),
            },
            field.to_string(),
        ),
        // PostgreSQL / standard `INTERVAL '3 days'`
        (sp::Expr::Value(v), None) => match v.value {
            sp::Value::SingleQuotedString(s) => {
                let mut it = s.split_whitespace();
                match (it.next(), it.next(), it.next()) {
                    (Some(a), Some(u), None) => (a.to_string(), u.to_string()),
                    _ => {
                        return no(
                            "interval",
                            "only a single `<number> <unit>` interval is in the IR",
                        );
                    }
                }
            }
            other => return no("interval", &head(&other)),
        },
        (other, _) => return no("interval", &head(&other)),
    };
    let numeric = !amount.is_empty()
        && amount
            .trim_start_matches('-')
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
        && amount
            .trim_start_matches('-')
            .chars()
            .any(|c| c.is_ascii_digit());
    if !numeric {
        return no("interval", "the interval amount is not a plain number");
    }
    let unit = match unit.to_ascii_lowercase().trim_end_matches('s') {
        "second" | "sec" => "secs",
        "minute" | "min" => "mins",
        "hour" => "hours",
        "day" => "days",
        "week" => "weeks",
        "month" | "mon" => "months",
        "year" => "years",
        other => return no("interval", &format!("unit `{other}` is not in the IR")),
    };
    Ok(Expr::Function(Function {
        name: "make_interval".to_string(),
        args: vec![FunctionArg::Named {
            name: unit.to_string(),
            value: Expr::Literal(Literal::Number(amount)),
        }],
        distinct: false,
        star: false,
        filter: None,
        over: None,
        order_by: vec![],
    }))
}

fn window_spec(w: sp::WindowSpec) -> R<Window> {
    let sp::WindowSpec {
        window_name,
        partition_by,
        order_by,
        window_frame,
    } = w;
    if window_name.is_some() {
        return no("named_window", "OVER (w ..) refinement of a named window");
    }
    let frame = match window_frame {
        None => None,
        Some(sp::WindowFrame {
            units,
            start_bound,
            end_bound,
        }) => {
            let bound = |b: sp::WindowFrameBound| -> R<FrameBound> {
                Ok(match b {
                    sp::WindowFrameBound::CurrentRow => FrameBound::CurrentRow,
                    sp::WindowFrameBound::Preceding(e) => {
                        FrameBound::Preceding(e.map(|x| expr(*x).map(Box::new)).transpose()?)
                    }
                    sp::WindowFrameBound::Following(e) => {
                        FrameBound::Following(e.map(|x| expr(*x).map(Box::new)).transpose()?)
                    }
                })
            };
            Some(Frame {
                units: match units {
                    sp::WindowFrameUnits::Rows => FrameUnits::Rows,
                    sp::WindowFrameUnits::Range => FrameUnits::Range,
                    sp::WindowFrameUnits::Groups => FrameUnits::Groups,
                },
                start: bound(start_bound)?,
                end: end_bound.map(bound).transpose()?,
            })
        }
    };
    Ok(Window {
        partition_by: partition_by.into_iter().map(expr).collect::<R<_>>()?,
        order_by: order_by.into_iter().map(order_item).collect::<R<_>>()?,
        frame,
    })
}

fn data_type_to_ir(t: &sp::DataType) -> R<SqlType> {
    use sp::DataType as D;
    Ok(match t {
        D::SmallInt(_) | D::Int2(_) => SqlType::SmallInt,
        D::Int(_) | D::Integer(_) | D::Int4(_) => SqlType::Integer,
        D::BigInt(_) | D::Int8(_) => SqlType::BigInt,
        D::Real | D::Float4 => SqlType::Real,
        D::Double(_) | D::DoublePrecision | D::Float8 => SqlType::Double,
        D::Numeric(info) | D::Decimal(info) => match info {
            sp::ExactNumberInfo::None => SqlType::Numeric {
                precision: None,
                scale: None,
            },
            sp::ExactNumberInfo::Precision(p) => SqlType::Numeric {
                precision: Some(*p as u32),
                scale: None,
            },
            sp::ExactNumberInfo::PrecisionAndScale(p, s) => SqlType::Numeric {
                precision: Some(*p as u32),
                scale: Some(*s as u32),
            },
        },
        D::Text => SqlType::Text,
        D::Signed | D::SignedInteger | D::Unsigned | D::UnsignedInteger => SqlType::BigInt,
        D::Char(None) | D::Character(None) => SqlType::Text,
        D::Datetime(None) => SqlType::Timestamp { with_tz: false },
        D::Varchar(None) | D::CharVarying(None) => SqlType::Varchar(None),
        D::Varchar(Some(sp::CharacterLength::IntegerLength { length, unit: None }))
        | D::CharVarying(Some(sp::CharacterLength::IntegerLength { length, unit: None })) => {
            SqlType::Varchar(Some(*length as u32))
        }
        D::Boolean | D::Bool => SqlType::Boolean,
        D::Date => SqlType::Date,
        D::Timestamp(None, tz) => SqlType::Timestamp {
            with_tz: match tz {
                sp::TimezoneInfo::None | sp::TimezoneInfo::WithoutTimeZone => false,
                sp::TimezoneInfo::WithTimeZone | sp::TimezoneInfo::Tz => true,
            },
        },
        D::Uuid => SqlType::Uuid,
        D::Bytea => SqlType::Bytes,
        D::JSON => SqlType::Json,
        D::JSONB => SqlType::Jsonb,
        other => return no("data_type", &format!("{other}")),
    })
}
