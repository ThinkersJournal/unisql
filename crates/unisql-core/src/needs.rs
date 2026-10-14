// SPDX-License-Identifier: MIT OR Apache-2.0
//! Target-free construct extraction: which constructs does a statement use?
//!
//! The ids are the same strings the writers record in a [`crate::Report`] (`like`, `fn:now`,
//! `upsert`, `join:left_join`, ...), so a gate can compare a statement's needs with a matrix row's
//! subject. This walker needs no target: it reads the IR shape, so it sees every construct even
//! where a particular target would stop at its first refusal.

use std::collections::BTreeSet;

use crate::ast::*;
use crate::print::is_numeric;

/// Every construct id the statement uses.
pub fn constructs(stmt: &Statement) -> BTreeSet<String> {
    let mut w = Walk::default();
    w.statement(stmt);
    w.out
}

#[derive(Default)]
struct Walk {
    out: BTreeSet<String>,
}

impl Walk {
    fn statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::With(w) => {
                self.add(match &*w.stmt {
                    Statement::Insert(_) => "with.insert",
                    Statement::Update(_) => "with.update",
                    _ => "with.delete",
                });
                if w.recursive {
                    self.add("recursive_cte");
                }
                for c in &w.ctes {
                    match &c.body {
                        CteBody::Query(q) => self.query(q),
                        CteBody::Dml(st) => {
                            self.add("cte.data_modifying");
                            self.statement(st);
                        }
                    }
                }
                self.statement(&w.stmt);
            }
            Statement::Query(q) => self.query(q),
            Statement::Insert(i) => {
                match &i.source {
                    InsertSource::Query(q) => self.query(q),
                    InsertSource::Values(rows) => rows.iter().flatten().for_each(|e| self.expr(e)),
                    InsertSource::DefaultValues => {}
                }
                if let Some(oc) = &i.on_conflict {
                    match &oc.action {
                        ConflictAction::DoNothing => self.add("upsert"),
                        ConflictAction::DoUpdate { set, where_ } => {
                            self.add(if where_.is_some() {
                                "upsert_where"
                            } else {
                                "upsert"
                            });
                            // A target that needs the conflict key refuses this form.
                            if oc.target.is_empty() {
                                self.add("upsert.no_conflict_key");
                            }
                            set.iter().for_each(|a| self.expr(&a.value));
                            if let Some(e) = where_ {
                                self.expr(e);
                            }
                        }
                    }
                }
                self.returning(&i.returning);
            }
            Statement::Update(u) => {
                u.set.iter().for_each(|a| self.expr(&a.value));
                if !u.returning.is_empty() {
                    self.add("returning.update");
                }
                if !u.from.is_empty() {
                    self.add("update_from");
                    u.from.iter().for_each(|t| self.table(t));
                }
                if let Some(e) = &u.where_ {
                    self.expr(e);
                }
                self.returning(&u.returning);
            }
            Statement::Delete(d) => {
                d.using.iter().for_each(|t| self.table(t));
                if let Some(e) = &d.where_ {
                    self.expr(e);
                }
                self.returning(&d.returning);
            }
        }
    }

    fn add(&mut self, c: impl Into<String>) {
        self.out.insert(c.into());
    }

    /// `make_interval(months|years => ..)`: month arithmetic differs between engines, so it is its
    /// own leaf and a target may refuse it while giving the fixed-length units.
    fn month_year(&mut self, f: &Function) {
        if f.name == "make_interval"
            && f.args.iter().any(|a| {
                matches!(a, FunctionArg::Named { name, .. } if name == "months" || name == "years")
            })
        {
            self.add("fn:make_interval.month_year");
        }
    }

    fn returning(&mut self, items: &[SelectItem]) {
        if !items.is_empty() {
            self.add("returning");
            self.items(items);
        }
    }

    fn items(&mut self, items: &[SelectItem]) {
        for it in items {
            if let SelectItem::Expr { expr, .. } = it {
                self.expr(expr);
            }
        }
    }

    fn query(&mut self, q: &Query) {
        if q.recursive {
            self.add("recursive_cte");
        }
        for c in &q.with {
            match &c.body {
                CteBody::Query(q) => self.query(q),
                CteBody::Dml(st) => {
                    self.add("cte.data_modifying");
                    self.statement(st);
                }
            }
        }
        self.set_expr(&q.body);
        if !q.order_by.is_empty() {
            self.add("order_by");
            self.add("null_ordering");
            for o in &q.order_by {
                self.expr(&o.expr);
            }
        }
        if let Some(l) = &q.limit {
            self.add("limit");
            self.expr(l);
        }
        if let Some(o) = &q.offset {
            self.add("offset");
            self.expr(o);
        }
    }

    fn set_expr(&mut self, s: &SetExpr) {
        match s {
            SetExpr::Select(sel) => self.select(sel),
            SetExpr::SetOp { left, right, .. } => {
                self.set_expr(left);
                self.set_expr(right);
            }
        }
    }

    fn select(&mut self, s: &Select) {
        if let Distinct::On(keys) = &s.distinct {
            self.add("distinct_on");
            if s.projection
                .iter()
                .any(|i| !matches!(i, SelectItem::Expr { .. }))
            {
                self.add("distinct_on.star_projection");
            }
            if s.lock.is_some() {
                self.add("distinct_on.row_lock");
            }
            // Targets without it emulate it with `row_number() OVER (PARTITION BY ..)`; that is a
            // dependency of the statement there, so it is named here (an over-approximation on
            // targets that have DISTINCT ON natively, where window functions exist anyway).
            self.add("fn:row_number");
            self.add("window_function");
            keys.iter().for_each(|e| self.expr(e));
        }
        self.items(&s.projection);
        s.from.iter().for_each(|t| self.table(t));
        if let Some(e) = &s.where_ {
            self.expr(e);
        }
        s.group_by.iter().for_each(|e| self.expr(e));
        if let Some(e) = &s.having {
            self.expr(e);
        }
        if let Some(l) = &s.lock {
            self.add(match l.strength {
                RowLock::Update => "row_lock",
                RowLock::Share => "row_lock.share",
            });
            if l.of.is_some() {
                self.add("row_lock.of");
            }
            match l.wait {
                LockWait::Block => {}
                LockWait::NoWait => self.add("row_lock.nowait"),
                LockWait::SkipLocked => self.add("row_lock.skip_locked"),
            }
        }
    }

    fn table(&mut self, t: &TableRef) {
        match t {
            TableRef::Table { .. } => {}
            TableRef::Subquery { query, .. } => self.query(query),
            TableRef::Join {
                left,
                right,
                kind,
                on,
            } => {
                self.add(match kind {
                    JoinKind::Inner => "join:inner_join",
                    JoinKind::Left => "join:left_join",
                    JoinKind::Right => "join:right_join",
                    JoinKind::Full => "join:full_join",
                    JoinKind::Cross => "join:cross_join",
                });
                self.table(left);
                self.table(right);
                if let Some(e) = on {
                    self.expr(e);
                }
            }
        }
    }

    /// Writers record `ordered_comparison` for every ordered comparison (Exact when an operand is
    /// numeric) and `text_comparison` for the ones that may compare text.
    fn compare(&mut self, numeric: bool, ordered: bool) {
        if ordered {
            self.add("ordered_comparison");
        }
        if !numeric {
            self.add("text_comparison");
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Column(_) | Expr::Param(_) => {}
            Expr::Literal(l) => {
                if matches!(l, Literal::Bool(_)) {
                    self.add("boolean_literal");
                }
            }
            Expr::Nested(x) => self.expr(x),
            Expr::Unary { expr, .. } => self.expr(expr),
            Expr::Binary { op, left, right } => {
                if matches!(&**right, Expr::Function(f) if f.name == "make_interval")
                    && matches!(op, BinaryOp::Plus | BinaryOp::Minus)
                {
                    self.add("interval_arithmetic");
                    // PostgreSQL itself spells it as the named-argument call, so both are needs.
                    self.add("fn:make_interval");
                    if let Expr::Function(f) = &**right {
                        self.month_year(f);
                    }
                    self.expr(left);
                    if let Expr::Function(f) = &**right {
                        for a in &f.args {
                            if let FunctionArg::Named { value, .. } = a {
                                self.expr(value);
                            }
                        }
                    }
                    return;
                }
                let numeric = is_numeric(left) || is_numeric(right);
                match op {
                    BinaryOp::Concat => self.add("concat"),
                    BinaryOp::Modulo => self.add("division"),
                    // An IR `/` has PostgreSQL's meaning (integer operands truncate), which a target
                    // with exact decimal division must spell as integer division.
                    BinaryOp::Divide => {
                        self.add("division");
                        self.add("division_integer");
                    }
                    BinaryOp::IntDivide => {
                        self.add("division");
                        self.add("division_integer");
                    }
                    BinaryOp::Eq | BinaryOp::NotEq => self.compare(numeric, false),
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        self.compare(numeric, true)
                    }
                    BinaryOp::JsonGet => self.add("json_get"),
                    BinaryOp::JsonGetText => self.add("json_get_text"),
                    _ => {}
                }
                self.expr(left);
                self.expr(right);
            }
            Expr::IsNull { expr, .. } => self.expr(expr),
            Expr::InList { expr, list, .. } => {
                self.expr(expr);
                list.iter().for_each(|x| self.expr(x));
            }
            Expr::InSubquery { expr, query, .. } => {
                self.expr(expr);
                self.query(query);
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                let numeric = is_numeric(low) || is_numeric(high) || is_numeric(expr);
                self.compare(numeric, true);
                self.expr(expr);
                self.expr(low);
                self.expr(high);
            }
            Expr::Like {
                expr,
                pattern,
                case_insensitive,
                escape,
                ..
            } => {
                self.add(if *case_insensitive { "ilike" } else { "like" });
                self.expr(expr);
                self.expr(pattern);
                if let Some(x) = escape {
                    self.expr(x);
                }
            }
            Expr::Case {
                operand,
                branches,
                else_,
            } => {
                if let Some(o) = operand {
                    self.expr(o);
                }
                for (w, t) in branches {
                    self.expr(w);
                    self.expr(t);
                }
                if let Some(x) = else_ {
                    self.expr(x);
                }
            }
            Expr::Cast { expr, ty, .. } => {
                self.add(format!("cast.to_{}", ty.leaf()));
                self.expr(expr);
            }
            Expr::Function(f) => {
                self.add(format!("fn:{}", f.name));
                self.month_year(f);
                if f.name == "make_interval" {
                    // Outside `ts +/- make_interval(..)`: no target translates it on its own.
                    self.add("fn:make_interval.standalone");
                }
                for a in &f.args {
                    match a {
                        FunctionArg::Expr(x) => self.expr(x),
                        FunctionArg::Named { value, .. } => {
                            self.add("named_arg");
                            self.expr(value);
                        }
                    }
                }
                if let Some(x) = &f.filter {
                    self.add("filter_clause");
                    self.expr(x);
                }
                if let Some(w) = &f.over {
                    self.add("window_function");
                    w.partition_by.iter().for_each(|x| self.expr(x));
                    w.order_by.iter().for_each(|o| self.expr(&o.expr));
                    if !w.order_by.is_empty() {
                        self.add("order_by");
                        self.add("null_ordering");
                    }
                }
                if !f.order_by.is_empty() {
                    self.add("order_by");
                    self.add("null_ordering");
                    self.add(format!("fn:{}.order_by", f.name));
                }
                f.order_by.iter().for_each(|o| self.expr(&o.expr));
            }
            Expr::Subquery(q) => self.query(q),
            Expr::Exists { query, .. } => self.query(query),
            Expr::AnyParam { expr, .. } => {
                self.add("array_param");
                self.expr(expr);
            }
            Expr::AtTimeZone { expr, zone } => {
                self.add("at_time_zone");
                self.expr(expr);
                self.expr(zone);
            }
            Expr::ArraySubquery(q) => {
                self.add("array_subquery");
                self.query(q);
            }
            Expr::Trim { expr, chars, .. } => {
                self.add("trim");
                // MySQL removes a repeated string where PostgreSQL removes a set: they agree only
                // for one plain character, so anything else is its own leaf.
                let single = match chars.as_deref() {
                    None => true,
                    Some(Expr::Literal(Literal::String(s))) => {
                        s.chars().count() == 1 && s != "'" && s != "\\"
                    }
                    Some(_) => false,
                };
                if !single {
                    self.add("trim.multi_char");
                }
                self.expr(expr);
                if let Some(c) = chars {
                    self.expr(c);
                }
            }
        }
    }
}
