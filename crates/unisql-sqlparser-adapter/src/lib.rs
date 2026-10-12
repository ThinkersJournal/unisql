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
use unisql_core::{Allowance, Read, ReadError, ReadNote};

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

fn statement(s: sp::Statement) -> R<Statement> {
    match s {
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
        let sp::With {
            with_token: _,
            recursive: rec,
            cte_tables,
        } = w;
        recursive = rec;
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
            ctes.push(Cte {
                name: ident(&alias.name),
                columns,
                query: query(*cq)?,
            });
        }
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
            if of.is_some() || nonblock.is_some() {
                return no(
                    "row_lock_option",
                    "FOR UPDATE OF / NOWAIT / SKIP LOCKED is not in the IR yet",
                );
            }
            lock = Some(match lock_type {
                sp::LockType::Update => RowLock::Update,
                sp::LockType::Share => RowLock::Share,
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
            if args.is_some()
                || !with_hints.is_empty()
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
        let on = match constraint {
            sp::JoinConstraint::On(e) => Some(expr(e)?),
            sp::JoinConstraint::None => None,
            sp::JoinConstraint::Using(_) => {
                return no("join_using", "JOIN .. USING is not in the IR yet");
            }
            sp::JoinConstraint::Natural => return no("natural_join", "NATURAL JOIN"),
        };
        cur = TableRef::Join {
            left: Box::new(cur),
            right: Box::new(table_factor(relation)?),
            kind,
            on,
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
        || ignore
        || table_alias.is_some()
        || overwrite
        || !asg.is_empty()
        || partitioned.is_some()
        || !after_columns.is_empty()
        || output.is_some()
        || replace_into
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
    let columns = columns.iter().map(single_name).collect::<R<Vec<_>>>()?;
    let source = match source {
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
    Ok(Statement::Insert(Insert {
        table,
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

fn expr(e: sp::Expr) -> R<Expr> {
    use sp::Expr as E;
    Ok(match e {
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
            Expr::Binary {
                op,
                left: boxed(*left)?,
                right: boxed(*right)?,
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
                return no(
                    "substring",
                    "MySQL SUBSTRING treats a start before 1 differently from PostgreSQL/SQLite",
                );
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
            return no("bare_function", &format!("`{base}` without parentheses"));
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
    if rules().name == "mysql" {
        match name.as_str() {
            "ifnull" => name = "coalesce".to_string(),
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
