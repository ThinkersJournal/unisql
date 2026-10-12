//! The shared printer: IR -> SQL text for any [`Target`].

use crate::ast::*;
use crate::report::{Options, Translation, Untranslatable};
use crate::target::{Ctx, Target};

type R<T> = Result<T, Untranslatable>;

/// Translate one statement for `target`. `source` names the dialect the IR was read from (used in
/// the refusal error and the report only).
pub fn write(
    stmt: &Statement,
    source: &str,
    target: &dyn Target,
    opts: &Options,
) -> Result<Translation, Untranslatable> {
    let mut ctx = Ctx::new(opts, source, target.name());
    let sql = Printer { t: target }.statement(stmt, &mut ctx)?;
    Ok(Translation { sql, param_count: ctx.max_param, param_order: ctx.param_order, report: ctx.report })
}

struct Printer<'t> {
    t: &'t dyn Target,
}

impl Printer<'_> {
    fn id(&self, s: &str) -> String {
        self.t.quote_ident(s)
    }

    fn name(&self, n: &Name) -> String {
        n.0.iter().map(|p| self.id(p)).collect::<Vec<_>>().join(".")
    }

    fn statement(&self, s: &Statement, c: &mut Ctx) -> R<String> {
        match s {
            Statement::Query(q) => self.query(q, c),
            Statement::Insert(i) => self.insert(i, c),
            Statement::Update(u) => self.update(u, c),
            Statement::Delete(d) => self.delete(d, c),
        }
    }

    fn query(&self, q: &Query, c: &mut Ctx) -> R<String> {
        let mut out = String::new();
        if !q.with.is_empty() {
            out.push_str("WITH ");
            let mut parts = Vec::new();
            for cte in &q.with {
                let cols = if cte.columns.is_empty() {
                    String::new()
                } else {
                    format!(
                        " ({})",
                        cte.columns.iter().map(|x| self.id(x)).collect::<Vec<_>>().join(", ")
                    )
                };
                parts.push(format!("{}{} AS ({})", self.id(&cte.name), cols, self.query(&cte.query, c)?));
            }
            out.push_str(&parts.join(", "));
            out.push(' ');
        }
        out.push_str(&self.set_expr(&q.body, c)?);
        if !q.order_by.is_empty() {
            out.push_str(" ORDER BY ");
            out.push_str(&self.order_items(&q.order_by, c)?);
        }
        let limit = q.limit.as_ref().map(|e| self.expr(e, c)).transpose()?;
        let offset = q.offset.as_ref().map(|e| self.expr(e, c)).transpose()?;
        out.push_str(&self.t.limit_offset(limit.as_deref(), offset.as_deref(), c)?);
        Ok(out)
    }

    fn order_items(&self, items: &[OrderItem], c: &mut Ctx) -> R<String> {
        let mut parts = Vec::new();
        for it in items {
            let mut s = self.expr(&it.expr, c)?;
            if it.desc {
                s.push_str(" DESC");
            }
            // The reader fills in the source default, so `nulls_first` is always `Some` here; a
            // hand-built IR with `None` is treated as the PostgreSQL default.
            let nf = it.nulls_first.unwrap_or(it.desc);
            s.push_str(&self.t.nulls_order(it.desc, nf, c)?);
            parts.push(s);
        }
        Ok(parts.join(", "))
    }

    fn set_expr(&self, s: &SetExpr, c: &mut Ctx) -> R<String> {
        match s {
            SetExpr::Select(sel) => self.select(sel, c),
            SetExpr::SetOp { op, all, left, right } => {
                let kw = match op {
                    SetOp::Union => "UNION",
                    SetOp::Intersect => "INTERSECT",
                    SetOp::Except => "EXCEPT",
                };
                Ok(format!(
                    "{} {}{} {}",
                    self.set_expr(left, c)?,
                    kw,
                    if *all { " ALL" } else { "" },
                    self.set_expr(right, c)?
                ))
            }
        }
    }

    fn select(&self, s: &Select, c: &mut Ctx) -> R<String> {
        let mut out = String::from("SELECT ");
        match &s.distinct {
            Distinct::No => {}
            Distinct::Yes => out.push_str("DISTINCT "),
            Distinct::On(exprs) => {
                self.t.distinct_on(c)?;
                let e = exprs.iter().map(|e| self.expr(e, c)).collect::<R<Vec<_>>>()?;
                out.push_str(&format!("DISTINCT ON ({}) ", e.join(", ")));
            }
        }
        out.push_str(&self.select_items(&s.projection, c)?);
        if !s.from.is_empty() {
            let f = s.from.iter().map(|t| self.table_ref(t, c)).collect::<R<Vec<_>>>()?;
            out.push_str(&format!(" FROM {}", f.join(", ")));
        }
        if let Some(w) = &s.where_ {
            out.push_str(&format!(" WHERE {}", self.expr(w, c)?));
        }
        if !s.group_by.is_empty() {
            let g = s.group_by.iter().map(|e| self.expr(e, c)).collect::<R<Vec<_>>>()?;
            out.push_str(&format!(" GROUP BY {}", g.join(", ")));
        }
        if let Some(h) = &s.having {
            out.push_str(&format!(" HAVING {}", self.expr(h, c)?));
        }
        if let Some(l) = s.lock {
            self.t.row_lock(l, c)?;
            out.push_str(match l {
                RowLock::Update => " FOR UPDATE",
                RowLock::Share => " FOR SHARE",
            });
        }
        Ok(out)
    }

    fn select_items(&self, items: &[SelectItem], c: &mut Ctx) -> R<String> {
        let mut parts = Vec::new();
        for it in items {
            parts.push(match it {
                SelectItem::Wildcard => "*".to_string(),
                SelectItem::QualifiedWildcard(n) => format!("{}.*", self.name(n)),
                SelectItem::Expr { expr, alias } => {
                    let e = self.expr(expr, c)?;
                    match alias {
                        Some(a) => format!("{e} AS {}", self.id(a)),
                        None => e,
                    }
                }
            });
        }
        Ok(parts.join(", "))
    }

    fn table_ref(&self, t: &TableRef, c: &mut Ctx) -> R<String> {
        match t {
            TableRef::Table { name, alias } => Ok(match alias {
                Some(a) => format!("{} AS {}", self.name(name), self.id(a)),
                None => self.name(name),
            }),
            TableRef::Subquery { query, alias } => {
                Ok(format!("({}) AS {}", self.query(query, c)?, self.id(alias)))
            }
            TableRef::Join { left, right, kind, on } => {
                let kw = match kind {
                    JoinKind::Inner => "INNER JOIN",
                    JoinKind::Left => "LEFT JOIN",
                    JoinKind::Right => "RIGHT JOIN",
                    JoinKind::Full => "FULL JOIN",
                    JoinKind::Cross => "CROSS JOIN",
                };
                c.note(
                    format!("join:{}", kw.to_lowercase().replace(' ', "_")),
                    self.t.join_tier(*kind),
                    "",
                    None,
                )?;
                let mut s = format!("{} {} {}", self.table_ref(left, c)?, kw, self.table_ref(right, c)?);
                if let Some(on) = on {
                    s.push_str(&format!(" ON {}", self.expr(on, c)?));
                }
                Ok(s)
            }
        }
    }

    fn returning(&self, items: &[SelectItem], c: &mut Ctx) -> R<String> {
        if items.is_empty() {
            return Ok(String::new());
        }
        self.t.returning(c)?;
        Ok(format!(" RETURNING {}", self.select_items(items, c)?))
    }

    fn insert(&self, i: &Insert, c: &mut Ctx) -> R<String> {
        let mut out = format!("INSERT INTO {}", self.name(&i.table));
        if !i.columns.is_empty() {
            out.push_str(&format!(
                " ({})",
                i.columns.iter().map(|x| self.id(x)).collect::<Vec<_>>().join(", ")
            ));
        }
        match &i.source {
            InsertSource::DefaultValues => out.push_str(" DEFAULT VALUES"),
            InsertSource::Query(q) => out.push_str(&format!(" {}", self.query(q, c)?)),
            InsertSource::Values(rows) => {
                let mut rs = Vec::new();
                for r in rows {
                    let es = r.iter().map(|e| self.expr(e, c)).collect::<R<Vec<_>>>()?;
                    rs.push(format!("({})", es.join(", ")));
                }
                out.push_str(&format!(" VALUES {}", rs.join(", ")));
            }
        }
        if let Some(oc) = &i.on_conflict {
            let has_where = matches!(&oc.action, ConflictAction::DoUpdate { where_: Some(_), .. });
            self.t.upsert(has_where, c)?;
            out.push_str(" ON CONFLICT");
            if !oc.target.is_empty() {
                out.push_str(&format!(
                    " ({})",
                    oc.target.iter().map(|x| self.id(x)).collect::<Vec<_>>().join(", ")
                ));
            }
            match &oc.action {
                ConflictAction::DoNothing => out.push_str(" DO NOTHING"),
                ConflictAction::DoUpdate { set, where_ } => {
                    out.push_str(" DO UPDATE SET ");
                    out.push_str(&self.assignments(set, c)?);
                    if let Some(w) = where_ {
                        out.push_str(&format!(" WHERE {}", self.expr(w, c)?));
                    }
                }
            }
        }
        out.push_str(&self.returning(&i.returning, c)?);
        Ok(out)
    }

    fn assignments(&self, set: &[Assignment], c: &mut Ctx) -> R<String> {
        let mut parts = Vec::new();
        for a in set {
            parts.push(format!("{} = {}", self.id(&a.column), self.expr(&a.value, c)?));
        }
        Ok(parts.join(", "))
    }

    fn update(&self, u: &Update, c: &mut Ctx) -> R<String> {
        let mut out = format!("UPDATE {}", self.name(&u.table));
        if let Some(a) = &u.alias {
            out.push_str(&format!(" AS {}", self.id(a)));
        }
        out.push_str(&format!(" SET {}", self.assignments(&u.set, c)?));
        if !u.from.is_empty() {
            c.note("update_from", self.t.update_from_tier(), "", None)?;
            let f = u.from.iter().map(|t| self.table_ref(t, c)).collect::<R<Vec<_>>>()?;
            out.push_str(&format!(" FROM {}", f.join(", ")));
        }
        if let Some(w) = &u.where_ {
            out.push_str(&format!(" WHERE {}", self.expr(w, c)?));
        }
        out.push_str(&self.returning(&u.returning, c)?);
        Ok(out)
    }

    fn delete(&self, d: &Delete, c: &mut Ctx) -> R<String> {
        let mut out = format!("DELETE FROM {}", self.name(&d.table));
        if let Some(a) = &d.alias {
            out.push_str(&format!(" AS {}", self.id(a)));
        }
        if !d.using.is_empty() {
            return Err(c.refuse(
                "delete_using",
                "DELETE .. USING is not represented in the first slice's writers",
                &["rewrite with a subquery: DELETE FROM t WHERE id IN (SELECT ..)"],
            ));
        }
        if let Some(w) = &d.where_ {
            out.push_str(&format!(" WHERE {}", self.expr(w, c)?));
        }
        out.push_str(&self.returning(&d.returning, c)?);
        Ok(out)
    }

    /// An operand of a binary/unary/predicate: parenthesise anything that is not atomic, so the
    /// output never depends on the target's precedence table.
    fn operand(&self, e: &Expr, c: &mut Ctx) -> R<String> {
        let s = self.expr(e, c)?;
        Ok(match e {
            Expr::Binary { .. }
            | Expr::Unary { .. }
            | Expr::IsNull { .. }
            | Expr::InList { .. }
            | Expr::InSubquery { .. }
            | Expr::Between { .. }
            | Expr::Like { .. } => format!("({s})"),
            Expr::Nested(inner) => self.operand(inner, c)?,
            _ => s,
        })
    }

    fn expr(&self, e: &Expr, c: &mut Ctx) -> R<String> {
        Ok(match e {
            Expr::Column(n) => self.name(n),
            Expr::Literal(l) => match l {
                Literal::Null => "NULL".to_string(),
                Literal::Bool(b) => self.t.bool_literal(*b, c)?,
                Literal::Number(n) => n.clone(),
                Literal::String(s) => self.t.string_literal(s),
            },
            Expr::Param(n) => {
                c.max_param = c.max_param.max(*n);
                c.param_order.push(*n);
                self.t.placeholder(*n)
            }
            Expr::Nested(inner) => self.expr(inner, c)?,
            Expr::Unary { op, expr } => {
                let o = self.operand(expr, c)?;
                match op {
                    UnaryOp::Not => format!("NOT {o}"),
                    UnaryOp::Minus => format!("-{o}"),
                    UnaryOp::Plus => format!("+{o}"),
                }
            }
            Expr::Binary { op, left, right } => {
                let sym = match op {
                    BinaryOp::Eq => "=",
                    BinaryOp::NotEq => "<>",
                    BinaryOp::Lt => "<",
                    BinaryOp::LtEq => "<=",
                    BinaryOp::Gt => ">",
                    BinaryOp::GtEq => ">=",
                    BinaryOp::And => "AND",
                    BinaryOp::Or => "OR",
                    BinaryOp::Plus => "+",
                    BinaryOp::Minus => "-",
                    BinaryOp::Multiply => "*",
                    BinaryOp::Divide => "/",
                    BinaryOp::Modulo => "%",
                    BinaryOp::Concat => "||",
                };
                self.t.binary_op(*op, is_numeric(left) || is_numeric(right), c)?;
                format!("{} {sym} {}", self.operand(left, c)?, self.operand(right, c)?)
            }
            Expr::IsNull { expr, negated } => {
                format!("{} IS {}NULL", self.operand(expr, c)?, if *negated { "NOT " } else { "" })
            }
            Expr::InList { expr, list, negated } => {
                let l = list.iter().map(|x| self.expr(x, c)).collect::<R<Vec<_>>>()?;
                format!(
                    "{} {}IN ({})",
                    self.operand(expr, c)?,
                    if *negated { "NOT " } else { "" },
                    l.join(", ")
                )
            }
            Expr::InSubquery { expr, query, negated } => format!(
                "{} {}IN ({})",
                self.operand(expr, c)?,
                if *negated { "NOT " } else { "" },
                self.query(query, c)?
            ),
            Expr::Between { expr, low, high, negated } => {
                self.t.between(is_numeric(low) || is_numeric(high) || is_numeric(expr), c)?;
                format!(
                "{} {}BETWEEN {} AND {}",
                self.operand(expr, c)?,
                if *negated { "NOT " } else { "" },
                self.operand(low, c)?,
                self.operand(high, c)?
                )
            }
            Expr::Like { expr, pattern, negated, case_insensitive } => {
                let l = self.operand(expr, c)?;
                let r = self.operand(pattern, c)?;
                self.t.like(&l, &r, *negated, *case_insensitive, c)?
            }
            Expr::Case { operand, branches, else_ } => {
                let mut s = String::from("CASE");
                if let Some(o) = operand {
                    s.push_str(&format!(" {}", self.operand(o, c)?));
                }
                for (w, t) in branches {
                    s.push_str(&format!(" WHEN {} THEN {}", self.expr(w, c)?, self.expr(t, c)?));
                }
                if let Some(e) = else_ {
                    s.push_str(&format!(" ELSE {}", self.expr(e, c)?));
                }
                s.push_str(" END");
                s
            }
            Expr::Cast { expr, ty } => {
                let inner = self.expr(expr, c)?;
                self.t.cast(&inner, ty, c)?
            }
            Expr::Function(f) => {
                let args = f
                    .args
                    .iter()
                    .map(|FunctionArg::Expr(a)| self.expr(a, c))
                    .collect::<R<Vec<_>>>()?;
                self.t.function(f, &args, c)?
            }
            Expr::Subquery(q) => format!("({})", self.query(q, c)?),
            Expr::Exists { query, negated } => format!(
                "{}EXISTS ({})",
                if *negated { "NOT " } else { "" },
                self.query(query, c)?
            ),
        })
    }
}

/// True when the expression is *syntactically* numeric, so comparing it involves no text collation.
/// Conservative: a bare column or parameter is unknown and returns false.
fn is_numeric(e: &Expr) -> bool {
    match e {
        Expr::Literal(Literal::Number(_)) => true,
        Expr::Nested(x) => is_numeric(x),
        Expr::Unary { op: UnaryOp::Minus | UnaryOp::Plus, expr } => is_numeric(expr),
        Expr::Binary {
            op: BinaryOp::Plus | BinaryOp::Minus | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Modulo,
            ..
        } => true,
        Expr::Function(f) => matches!(f.name.as_str(), "count" | "sum" | "avg" | "length" | "char_length" | "abs"),
        Expr::Cast { ty, .. } => matches!(
            ty,
            SqlType::SmallInt | SqlType::Integer | SqlType::BigInt | SqlType::Real | SqlType::Double | SqlType::Numeric { .. }
        ),
        _ => false,
    }
}
