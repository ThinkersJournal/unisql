def sub(p, a, b, count=1):
    s = open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')
    assert s.count(a) == count, (p, s.count(a), a[:100])
    open(p, 'w', encoding='utf-8', newline='').write(s.replace(a, b))


H = 'crates/unisql-differential/src/lib.rs'
sub(H, "use unisql::{Allowance, Options, Tier};", "use unisql::{Allowance, Options, ParamBinding, Tier};")
sub(H, "    Int(i64),\n    Real(f64),\n    Text(&'static str),\n}", """    Int(i64),
    Real(f64),
    Text(&'static str),
    /// An array value; the harness binds it as the target's [`ParamBinding`] requires.
    IntArray(Vec<i64>),
    TextArray(Vec<&'static str>),
    /// JSON text (what an array becomes on SQLite and MySQL).
    Json(String),
}""")
sub(H, "            (Type::FLOAT8, Param::Null) => Box::new(None::<f64>),", """            (Type::FLOAT8, Param::Null) => Box::new(None::<f64>),
            (Type::INT4_ARRAY, Param::IntArray(v)) => Box::new(v.iter().map(|x| *x as i32).collect::<Vec<i32>>()),
            (Type::INT8_ARRAY, Param::IntArray(v)) => Box::new(v.clone()),
            (Type::TEXT_ARRAY | Type::VARCHAR_ARRAY, Param::TextArray(v)) => {
                Box::new(v.iter().map(|s| s.to_string()).collect::<Vec<String>>())
            }
            (Type::INT4_ARRAY, Param::Null) => Box::new(None::<Vec<i32>>),
            (Type::INT8_ARRAY, Param::Null) => Box::new(None::<Vec<i64>>),
            (Type::TEXT_ARRAY | Type::VARCHAR_ARRAY, Param::Null) => Box::new(None::<Vec<String>>),""")
sub(H, """                    Param::Real(r) => rusqlite::types::Value::Real(*r),""", """                    Param::Real(r) => rusqlite::types::Value::Real(*r),
                    Param::Json(j) => rusqlite::types::Value::Text(j.clone()),
                    Param::IntArray(_) | Param::TextArray(_) => rusqlite::types::Value::Null,""")
sub(H, """                Param::Real(r) => mysql::Value::Double(*r),""", """                Param::Real(r) => mysql::Value::Double(*r),
                Param::Json(j) => mysql::Value::Bytes(j.as_bytes().to_vec()),
                Param::IntArray(_) | Param::TextArray(_) => mysql::Value::NULL,""")
sub(H, """fn target_params(target: &str, params: &[Param], order: &[u32]) -> Vec<Param> {
    if matches!(target, "mysql" | "mariadb") {
        order.iter().map(|n| params[(*n - 1) as usize].clone()).collect()
    } else {
        params.to_vec()
    }
}""", """fn target_params(target: &str, params: &[Param], order: &[u32], bindings: &[ParamBinding]) -> Vec<Param> {
    let conv = |n: usize| -> Param {
        let p = &params[n];
        match (p, bindings.get(n).copied().unwrap_or(ParamBinding::Scalar)) {
            (Param::IntArray(v), ParamBinding::JsonArrayText) => {
                Param::Json(format!("[{}]", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")))
            }
            (Param::TextArray(v), ParamBinding::JsonArrayText) => Param::Json(format!(
                "[{}]",
                v.iter()
                    .map(|s| format!("\\"{}\\"", s.replace('\\\\', "\\\\\\\\").replace('"', "\\\\\\"")))
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            _ => p.clone(),
        }
    };
    if matches!(target, "mysql" | "mariadb") {
        order.iter().map(|n| conv((*n - 1) as usize)).collect()
    } else {
        (0..params.len()).map(conv).collect()
    }
}""")
sub(H, "let tparams = target_params(tgt, &c.params, &t.param_order);", "let tparams = target_params(tgt, &c.params, &t.param_order, &t.param_bindings);", count=2)

C = 'crates/unisql-differential/src/bin/corpus.rs'
sub(C, "    Allowance::AggregateStringTruncation,\n];", "    Allowance::AggregateStringTruncation,\n    Allowance::ArrayNullSemantics,\n];")
print('ok')
