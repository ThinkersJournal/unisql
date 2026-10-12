def sub(p, a, b, count=1):
    s = open(p, encoding='utf-8', newline='').read().replace('\r\n', '\n')
    assert s.count(a) == count, (p, s.count(a), a[:100])
    open(p, 'w', encoding='utf-8', newline='').write(s.replace(a, b))


# ------------------------------------------------------------- pg -> sqlite
T = 'crates/unisql-differential/tests/postgres_to_sqlite.rs'
sub(T, "const COLL: &[A] = &[A::TextCollation];", "const COLL: &[A] = &[A::TextCollation];\nconst ARR: &[A] = &[A::ArrayNullSemantics];")
sub(T, '        // ---- milestone B: DISTINCT ON emulation', '''        // ---- array parameters: `= ANY($1)` binds as JSON text on SQLite (ParamBinding::JsonArrayText)
        p(c("any_int_array", "SELECT id FROM users WHERE id = ANY($1)", ARR), vec![Param::IntArray(vec![1, 3, 5, 9])]),
        p(c("all_ne_int_array", "SELECT id FROM users WHERE id <> ALL($1)", ARR), vec![Param::IntArray(vec![1, 3])]),
        p(c("any_text_array", "SELECT id FROM users WHERE city = ANY($1)", ARR), vec![Param::TextArray(vec!["Oslo", "Bergen"])]),
        p(c("any_cast_hint", "SELECT id FROM users WHERE id = ANY($1::int[])", ARR), vec![Param::IntArray(vec![2, 4])]),
        p(c("any_empty_array", "SELECT id FROM users WHERE id = ANY($1)", ARR), vec![Param::IntArray(vec![])]),
        p(c("all_ne_empty_array", "SELECT id FROM users WHERE id <> ALL($1)", ARR), vec![Param::IntArray(vec![])]),
        p(c("any_with_scalar", "SELECT id FROM users WHERE id = ANY($1) AND age > $2", &[A::ArrayNullSemantics, A::TextCollation]),
            vec![Param::IntArray(vec![1, 3, 5]), Param::Int(26)]),
        c("any_array_literal", "SELECT id FROM users WHERE id = ANY(ARRAY[1, 2])", &[]),
        p(exp(c("gate_array", "SELECT id FROM users WHERE id = ANY($1)", &[]), Expect::NeedsAllowance(A::ArrayNullSemantics)),
            vec![Param::IntArray(vec![1])]),
        p(exp(c("diverge_null_array", "SELECT id FROM users WHERE NOT (id = ANY($1))", ARR), Expect::Diverges), vec![Param::Null]),
        exp(c("ir_any_lt", "SELECT id FROM users WHERE id < ANY($1)", ARR), Expect::NotInIr("any_operator")),
        // ---- milestone B: DISTINCT ON emulation''')

# ------------------------------------------------------------- pg -> mysql / mariadb
M = 'crates/unisql-differential/tests/postgres_to_mysql.rs'
sub(M, "    A::AggregateStringTruncation,\n];", "    A::AggregateStringTruncation,\n    A::ArrayNullSemantics,\n];")
sub(M, '        // ---- the allowance gate: without it, no text', '''        // ---- array parameters bind as JSON text (ParamBinding::JsonArrayText) via JSON_CONTAINS
        p(c("any_int_array", "SELECT id FROM users WHERE id = ANY($1)"), vec![Param::IntArray(vec![1, 3, 5, 9])]),
        p(c("all_ne_int_array", "SELECT id FROM users WHERE id <> ALL($1)"), vec![Param::IntArray(vec![1, 3])]),
        p(c("any_text_array", "SELECT id FROM users WHERE city = ANY($1)"), vec![Param::TextArray(vec!["Oslo", "Bergen"])]),
        p(c("any_empty_array", "SELECT id FROM users WHERE id = ANY($1)"), vec![Param::IntArray(vec![])]),
        p(c("all_ne_empty_array", "SELECT id FROM users WHERE id <> ALL($1)"), vec![Param::IntArray(vec![])]),
        p(c("any_with_scalar_reorder", "SELECT id FROM users WHERE age > $2 AND id = ANY($1) AND age < $2 + 100"),
            vec![Param::IntArray(vec![1, 3, 5]), Param::Int(26)]),
        c("any_array_literal", "SELECT id FROM users WHERE id = ANY(ARRAY[1, 2])"),
        p(exp(only(c("gate_array", "SELECT id FROM users WHERE id = ANY($1)"), &[]), Expect::NeedsAllowance(A::ArrayNullSemantics)),
            vec![Param::IntArray(vec![1])]),
        // ---- the allowance gate: without it, no text''')

# ------------------------------------------------------------- docs for the result type
print('ok')
