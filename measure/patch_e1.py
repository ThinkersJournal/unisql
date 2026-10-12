def sub(p, a, b, count=1):
    s = open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')
    assert s.count(a) == count, (p, s.count(a), a[:100])
    open(p, 'w', encoding='utf-8', newline='').write(s.replace(a, b))


# ------------------------------------------------------------------ IR
AST = 'crates/unisql-core/src/ast.rs'
sub(AST, """    /// `OVER (..)`: makes the call a window function.
    pub over: Option<Window>,
}""", """    /// `OVER (..)`: makes the call a window function.
    pub over: Option<Window>,
    /// `agg(args ORDER BY ..)`: the order in which an aggregate consumes its input.
    pub order_by: Vec<OrderItem>,
}""")
sub('crates/unisql-core/src/rewrite.rs', "            filter: None,\n            over: Some(Window {", "            filter: None,\n            order_by: vec![],\n            over: Some(Window {")

# ------------------------------------------------------------------ target hooks
TGT = 'crates/unisql-core/src/target.rs'
sub(TGT, "    pub in_upsert: bool,\n}", "    pub in_upsert: bool,\n    /// Printed `ORDER BY` of the aggregate call being printed, for the function hook to consume.\n    /// A hook that does not `take()` it cannot honour the order, and the printer then refuses.\n    pub agg_order: Option<String>,\n}")
sub(TGT, "param_order: Vec::new(), in_upsert: false }", "param_order: Vec::new(), in_upsert: false, agg_order: None }")
sub(TGT, "type R<T> = Result<T, Untranslatable>;\n", """type R<T> = Result<T, Untranslatable>;

/// The kind of statement a clause belongs to (some clauses exist only for some kinds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StmtKind {
    Insert,
    Update,
    Delete,
}
""")
sub(TGT, """    fn returning(&self, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "", None)
    }""", """    fn returning(&self, _kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        ctx.note("returning", Tier::Exact, "", None)
    }""")
sub(TGT, """        ctx.note(format!("fn:{}", f.name), Tier::Exact, "", None)?;
        Ok(generic_call(&f.name, args, f.distinct, f.star))
    }""", """        ctx.note(format!("fn:{}", f.name), Tier::Exact, "", None)?;
        let order = ctx.agg_order.take();
        Ok(generic_call_ordered(&f.name, args, f.distinct, f.star, order.as_deref()))
    }""")
sub(TGT, "pub fn generic_call(name: &str, args: &[String], distinct: bool, star: bool) -> String {", """pub fn generic_call(name: &str, args: &[String], distinct: bool, star: bool) -> String {
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
    let ord = order.map(|o| format!(" ORDER BY {o}")).unwrap_or_default();""")
sub(TGT, """    format!(
        "{}({}{})",
        name.to_uppercase(),
        if distinct { "DISTINCT " } else { "" },
        args.join(", ")
    )
}""", """    format!(
        "{}({}{}{})",
        name.to_uppercase(),
        if distinct { "DISTINCT " } else { "" },
        args.join(", "),
        ord
    )
}""")

# ------------------------------------------------------------------ printer
PRN = 'crates/unisql-core/src/print.rs'
sub(PRN, "use crate::target::{Ctx, Escape, Target};", "use crate::target::{Ctx, Escape, StmtKind, Target};")
sub(PRN, """    fn returning(&self, items: &[SelectItem], c: &mut Ctx) -> R<String> {
        if items.is_empty() {
            return Ok(String::new());
        }
        self.t.returning(c)?;""", """    fn returning(&self, items: &[SelectItem], kind: StmtKind, c: &mut Ctx) -> R<String> {
        if items.is_empty() {
            return Ok(String::new());
        }
        self.t.returning(kind, c)?;""")
sub(PRN, "        out.push_str(&self.returning(&i.returning, c)?);", "        out.push_str(&self.returning(&i.returning, StmtKind::Insert, c)?);")
sub(PRN, "        out.push_str(&self.returning(&u.returning, c)?);", "        out.push_str(&self.returning(&u.returning, StmtKind::Update, c)?);")
sub(PRN, "        out.push_str(&self.returning(&d.returning, c)?);", "        out.push_str(&self.returning(&d.returning, StmtKind::Delete, c)?);")
sub(PRN, """                let mut s = self.t.function(f, &args, c)?;
                if let Some(flt) = &f.filter {""", """                if !f.order_by.is_empty() {
                    c.agg_order = Some(self.order_items(&f.order_by, c)?);
                }
                let mut s = self.t.function(f, &args, c);
                if c.agg_order.take().is_some() && s.is_ok() {
                    return Err(c.refuse(
                        &format!("fn:{}", f.name),
                        "this target cannot honour an ORDER BY inside this aggregate",
                        &["order in a subquery", "use string_agg, which is translated"],
                    ));
                }
                let mut s = s?;
                if let Some(flt) = &f.filter {""")

# ------------------------------------------------------------------ dialects
PG = 'crates/unisql-dialect-postgres/src/lib.rs'
sub(PG, "use unisql_core::target::{generic_call, Ctx, PORTABLE_FUNCTIONS};", "use unisql_core::target::{generic_call_ordered, Ctx, PORTABLE_FUNCTIONS};")
sub(PG, "        Ok(generic_call(n, args, f.distinct, f.star))", "        let order = ctx.agg_order.take();\n        Ok(generic_call_ordered(n, args, f.distinct, f.star, order.as_deref()))")

SL = 'crates/unisql-dialect-sqlite/src/lib.rs'
sub(SL, "use unisql_core::target::{generic_call, Ctx, Escape};", "use unisql_core::target::{generic_call, generic_call_ordered, Ctx, Escape, StmtKind};")
sub(SL, """            ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
            return Ok(generic_call(n, args, f.distinct, f.star));
        }
        if f.over.is_some()""", """            ctx.note(format!("fn:{n}"), Tier::Exact, "", None)?;
            let order = ctx.agg_order.take();
            return Ok(generic_call_ordered(n, args, f.distinct, f.star, order.as_deref()));
        }
        if f.over.is_some()""")
sub(SL, """                ctx.note(
                    "fn:string_agg",
                    Tier::Emulated,
                    "-> group_concat(value, separator); element order unspecified on both",
                    None,
                )?;
                Ok(generic_call("group_concat", args, f.distinct, false))""", """                ctx.note(
                    "fn:string_agg",
                    Tier::Emulated,
                    "-> group_concat(value, separator); an ORDER BY inside needs SQLite >= 3.44",
                    None,
                )?;
                let order = ctx.agg_order.take();
                Ok(generic_call_ordered("group_concat", args, f.distinct, false, order.as_deref()))""")
sub(SL, "    fn returning(&self, ctx: &mut Ctx) -> R<()> {", "    fn returning(&self, _kind: StmtKind, ctx: &mut Ctx) -> R<()> {")

MY = 'crates/unisql-dialect-mysql/src/lib.rs'
sub(MY, "use unisql_core::target::{Ctx, Escape};", "use unisql_core::target::{Ctx, Escape, StmtKind};")
sub(MY, """                Ok(format!(
                    "GROUP_CONCAT({}{} SEPARATOR {})",
                    if f.distinct { "DISTINCT " } else { "" },
                    args[0],
                    args[1]
                ))""", """                let order = ctx.agg_order.take().map(|o| format!(" ORDER BY {o}")).unwrap_or_default();
                Ok(format!(
                    "GROUP_CONCAT({}{}{} SEPARATOR {})",
                    if f.distinct { "DISTINCT " } else { "" },
                    args[0],
                    order,
                    args[1]
                ))""")
sub(MY, """    fn returning(&self, ctx: &mut Ctx) -> R<()> {
        match self.flavor {
            Flavor::MariaDb => ctx.note(
                "returning",
                Tier::Exact,
                "MariaDB >= 10.5 for INSERT and DELETE (UPDATE .. RETURNING is not supported)",
                None,
            ),""", """    fn returning(&self, kind: StmtKind, ctx: &mut Ctx) -> R<()> {
        match (self.flavor, kind) {
            (Flavor::MariaDb, StmtKind::Update) => Err(ctx.refuse(
                "returning",
                "MariaDB supports RETURNING on INSERT and DELETE only, not on UPDATE",
                &["run the UPDATE, then SELECT the rows back in the same transaction"],
            )),
            (Flavor::MariaDb, _) => ctx.note(
                "returning",
                Tier::Exact,
                "MariaDB >= 10.5 for INSERT and DELETE",
                None,
            ),""")
sub(MY, """            Flavor::MySql => Err(ctx.refuse(
                "returning",""", """            (Flavor::MySql, _) => Err(ctx.refuse(
                "returning",""")

# ------------------------------------------------------------------ reader (aggregate ORDER BY)
RD = 'crates/unisql-sqlparser-adapter/src/lib.rs'
sub(RD, """    if !clauses.is_empty() {
        return no("function_clause", "ORDER BY/LIMIT/SEPARATOR inside a function call");
    }""", """    let mut agg_order = Vec::new();
    for cl in clauses {
        match cl {
            sp::FunctionArgumentClause::OrderBy(items) => {
                for it in items {
                    agg_order.push(order_item(it)?);
                }
            }
            other => return no("function_clause", &format!("{} inside a function call", head(&other))),
        }
    }""")
sub(RD, "Expr::Function(Function { name: \"substr\".into(), args, distinct: false, star: false, filter: None, over: None })",
    "Expr::Function(Function {\n                name: \"substr\".into(),\n                args,\n                distinct: false,\n                star: false,\n                filter: None,\n                over: None,\n                order_by: vec![],\n            })")
sub(RD, """                    star: false,
                    filter: None,
                    over: None,
                }));""", """                    star: false,
                    filter: None,
                    over: None,
                    order_by: vec![],
                }));""")
sub(RD, "    Ok(Expr::Function(Function { name, args: out, distinct, star, filter, over }))",
    "    Ok(Expr::Function(Function { name, args: out, distinct, star, filter, over, order_by: agg_order }))")
print('ok')
