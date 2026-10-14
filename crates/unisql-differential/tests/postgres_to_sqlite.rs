// SPDX-License-Identifier: MIT OR Apache-2.0
//! PostgreSQL -> SQLite differential suite (first slice). Needs a reachable PostgreSQL
//! (`UNISQL_PG_URL`, default 127.0.0.1:55432, database created with `--locale=C`); fails loudly if not.
//!
//! The fixture schema is hand-written per engine. That is deliberate: the schema/type codec is part
//! of the portable unit and is *not* produced by statement translation (docs/MEASUREMENT.md, oracle
//! finding). Booleans are `boolean` on PostgreSQL and `integer 0/1` on SQLite.

use std::fmt::Write as _;

use unisql::Allowance as A;
use unisql_differential::{Case, Engines, Expect, Param, run_case};

const PG: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active boolean NOT NULL, score double precision, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (key text PRIMARY KEY, value text, n integer);
CREATE TABLE docs (id integer PRIMARY KEY, data jsonb);
CREATE TABLE events (id integer PRIMARY KEY, at timestamptz NOT NULL);
INSERT INTO docs VALUES (1,'{"a":"x","n":5,"t":true,"o":{"k":"deep"}}'),(2,'{"a":null,"n":7}');
INSERT INTO events VALUES (1, now() - interval '10 seconds'),(2, now() - interval '120 seconds'),(3, now() - interval '3600 seconds');
INSERT INTO users VALUES
 (1,'alice','alice@example.com',30,true,9.5,'Oslo'),
 (2,'Bob','bob@example.com',25,false,7.25,'Bergen'),
 (3,'carol',NULL,41,true,NULL,'Oslo'),
 (4,'dave','Dave@Example.com',NULL,true,3.0,NULL),
 (5,'Eve_X','eve@example.com',25,false,8.0,'Bergen'),
 (6,'frank%','f@x.org',60,true,1.5,'Trondheim');
INSERT INTO orders VALUES (1,1,100,'paid'),(2,1,250,'paid'),(3,2,75,'open'),(4,3,0,'void'),(5,3,500,'paid'),(6,5,30,'open');
INSERT INTO kv VALUES ('a','1',1),('b',NULL,2),('c','x',NULL),('Cap','z',3);
"#;

const LITE: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active integer NOT NULL, score real, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (key text PRIMARY KEY, value text, n integer);
CREATE TABLE docs (id integer PRIMARY KEY, data text);
CREATE TABLE events (id integer PRIMARY KEY, at text NOT NULL);
INSERT INTO docs VALUES (1,'{"a":"x","n":5,"t":true,"o":{"k":"deep"}}'),(2,'{"a":null,"n":7}');
INSERT INTO events VALUES (1, datetime('now','-10 seconds')),(2, datetime('now','-120 seconds')),(3, datetime('now','-3600 seconds'));
INSERT INTO users VALUES
 (1,'alice','alice@example.com',30,1,9.5,'Oslo'),
 (2,'Bob','bob@example.com',25,0,7.25,'Bergen'),
 (3,'carol',NULL,41,1,NULL,'Oslo'),
 (4,'dave','Dave@Example.com',NULL,1,3.0,NULL),
 (5,'Eve_X','eve@example.com',25,0,8.0,'Bergen'),
 (6,'frank%','f@x.org',60,1,1.5,'Trondheim');
INSERT INTO orders VALUES (1,1,100,'paid'),(2,1,250,'paid'),(3,2,75,'open'),(4,3,0,'void'),(5,3,500,'paid'),(6,5,30,'open');
INSERT INTO kv VALUES ('a','1',1),('b',NULL,2),('c','x',NULL),('Cap','z',3);
"#;

const COLL: &[A] = &[A::TextCollation];
const ARR: &[A] = &[A::ArrayNullSemantics];

fn c(name: &'static str, sql: &'static str, allow: &[A]) -> Case {
    Case {
        name,
        sql,
        params: vec![],
        allow: allow.to_vec(),
        ordered: false,
        after: None,
        hold: None,
        expect: Expect::Same,
    }
}
fn ord(mut x: Case) -> Case {
    x.ordered = true;
    x
}
fn p(mut x: Case, params: Vec<Param>) -> Case {
    x.params = params;
    x
}
fn after(mut x: Case, q: &'static str) -> Case {
    x.after = Some(q);
    x
}
fn exp(mut x: Case, e: Expect) -> Case {
    x.expect = e;
    x
}

fn cases() -> Vec<Case> {
    let like = [A::LikeCaseSensitivityIsConnectionSetting];
    let ilike = [
        A::LikeCaseSensitivityIsConnectionSetting,
        A::AsciiOnlyCaseFolding,
    ];
    let kvs = "SELECT key, value, n FROM kv ORDER BY key";
    vec![
        // ---- exact core
        p(
            c(
                "select_param",
                "SELECT id, name FROM users WHERE id = $1",
                &[],
            ),
            vec![Param::Int(3)],
        ),
        c(
            "select_equals_text",
            "SELECT id FROM users WHERE city = 'Oslo' AND active = TRUE",
            &[A::BooleanAsInteger],
        ),
        p(
            c(
                "param_reuse_reorder",
                "SELECT id FROM users WHERE id = $2 OR id = $1 OR id = $2",
                &[],
            ),
            vec![Param::Int(1), Param::Int(3)],
        ),
        c(
            "is_null",
            "SELECT id FROM users WHERE age IS NULL OR city IS NULL",
            &[],
        ),
        c(
            "in_list",
            "SELECT id FROM users WHERE id IN (1, 3, 5) AND id NOT IN (5)",
            &[],
        ),
        c(
            "between",
            "SELECT id FROM users WHERE age BETWEEN 25 AND 41",
            COLL,
        ),
        c(
            "comparison_ops",
            "SELECT id FROM users WHERE age >= 30 AND age < 60 AND id <> 3",
            COLL,
        ),
        c(
            "arith",
            "SELECT id, age * 2 + 1 AS a FROM users WHERE age IS NOT NULL",
            &[],
        ),
        c(
            "case_when",
            "SELECT id, CASE WHEN age >= 40 THEN 'senior' WHEN age IS NULL THEN 'unknown' ELSE 'adult' END AS k FROM users",
            COLL,
        ),
        c(
            "case_operand",
            "SELECT id, CASE city WHEN 'Oslo' THEN 1 WHEN 'Bergen' THEN 2 ELSE 0 END FROM users",
            &[],
        ),
        c(
            "coalesce_nullif",
            "SELECT id, coalesce(city, 'none'), nullif(age, 25) FROM users",
            &[],
        ),
        c(
            "string_funcs",
            "SELECT id, length(name), replace(name, 'a', 'A'), ltrim('  x') FROM users",
            &[],
        ),
        c(
            "concat_op",
            "SELECT name || '-' || coalesce(city, '?') FROM users",
            &[],
        ),
        c(
            "aggregates",
            "SELECT count(*), count(status), sum(amount), min(amount), max(amount) FROM orders",
            COLL,
        ),
        c(
            "count_distinct",
            "SELECT count(DISTINCT user_id) FROM orders",
            &[],
        ),
        c(
            "avg_cast",
            "SELECT CAST(avg(amount) AS DOUBLE PRECISION) FROM orders",
            &[A::LenientCast],
        ),
        c(
            "group_having",
            "SELECT user_id, count(*) AS n, sum(amount) AS total FROM orders GROUP BY user_id HAVING sum(amount) > 100",
            COLL,
        ),
        c("distinct", "SELECT DISTINCT city FROM users", &[]),
        c(
            "inner_join",
            "SELECT u.name, o.amount FROM users u JOIN orders o ON o.user_id = u.id WHERE o.status = 'paid'",
            &[],
        ),
        c(
            "left_join",
            "SELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id",
            &[],
        ),
        c(
            "in_subquery",
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE amount > 100)",
            COLL,
        ),
        c(
            "exists",
            "SELECT name FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND o.status = 'open')",
            &[],
        ),
        c(
            "scalar_subquery",
            "SELECT id, (SELECT count(*) FROM orders o WHERE o.user_id = u.id) FROM users u",
            &[],
        ),
        c(
            "union_all",
            "SELECT id FROM users WHERE id < 3 UNION ALL SELECT id FROM orders WHERE id < 3",
            &[],
        ),
        c(
            "union_distinct",
            "SELECT user_id FROM orders UNION SELECT id FROM users WHERE id > 4",
            &[],
        ),
        c(
            "intersect",
            "SELECT id FROM users INTERSECT SELECT user_id FROM orders",
            &[],
        ),
        c(
            "except",
            "SELECT id FROM users EXCEPT SELECT user_id FROM orders",
            &[],
        ),
        c(
            "cte",
            "WITH big AS (SELECT user_id, sum(amount) AS total FROM orders GROUP BY user_id) SELECT u.name, big.total FROM users u JOIN big ON big.user_id = u.id",
            &[],
        ),
        // ---- ordering, null ordering, limit/offset
        ord(c(
            "order_asc_nulls",
            "SELECT id, age FROM users ORDER BY age, id",
            COLL,
        )),
        ord(c(
            "order_desc_nulls",
            "SELECT id, age FROM users ORDER BY age DESC, id",
            COLL,
        )),
        ord(c(
            "order_nulls_first",
            "SELECT id, age FROM users ORDER BY age NULLS FIRST, id",
            COLL,
        )),
        ord(c(
            "order_desc_nulls_last",
            "SELECT id, age FROM users ORDER BY age DESC NULLS LAST, id",
            COLL,
        )),
        ord(c(
            "limit_offset",
            "SELECT id FROM users ORDER BY id LIMIT 2 OFFSET 1",
            COLL,
        )),
        ord(c(
            "offset_only",
            "SELECT id FROM users ORDER BY id OFFSET 4",
            COLL,
        )),
        ord(c(
            "order_text",
            "SELECT name FROM users ORDER BY name",
            COLL,
        )),
        c(
            "text_less_than",
            "SELECT id FROM users WHERE name < 'Z' AND name > 'A'",
            COLL,
        ),
        // ---- LIKE family
        p(
            c(
                "like_param",
                "SELECT name FROM users WHERE name LIKE $1",
                &like,
            ),
            vec![Param::Text("a%")],
        ),
        c(
            "like_case_sensitive",
            "SELECT name FROM users WHERE name LIKE 'bob'",
            &like,
        ),
        c(
            "like_escape_percent",
            "SELECT name FROM users WHERE name LIKE 'frank\\%'",
            &like,
        ),
        c(
            "like_underscore",
            "SELECT name FROM users WHERE name LIKE 'Eve\\_X'",
            &like,
        ),
        c(
            "not_like",
            "SELECT id FROM users WHERE name NOT LIKE '%a%'",
            &like,
        ),
        p(
            c("ilike", "SELECT id FROM users WHERE email ILIKE $1", &ilike),
            vec![Param::Text("%EXAMPLE.COM")],
        ),
        // ---- DML
        after(
            p(
                c(
                    "insert_values",
                    "INSERT INTO users (id, name, active) VALUES ($1, $2, TRUE)",
                    &[A::BooleanAsInteger],
                ),
                vec![Param::Int(7), Param::Text("gus")],
            ),
            "SELECT id, name, active FROM users WHERE id = 7",
        ),
        c(
            "insert_returning",
            "INSERT INTO kv (key, value, n) VALUES ('d', 'w', 9) RETURNING key, n",
            &[],
        ),
        after(
            p(
                c(
                    "upsert_do_update",
                    "INSERT INTO kv (key, value, n) VALUES ($1, $2, $3) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, n = EXCLUDED.n",
                    &[],
                ),
                vec![Param::Text("a"), Param::Text("new"), Param::Int(11)],
            ),
            kvs,
        ),
        after(
            p(
                c(
                    "upsert_insert_path",
                    "INSERT INTO kv (key, value, n) VALUES ($1, $2, $3) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                    &[],
                ),
                vec![Param::Text("z"), Param::Text("fresh"), Param::Int(1)],
            ),
            kvs,
        ),
        after(
            c(
                "upsert_do_nothing",
                "INSERT INTO kv (key, value, n) VALUES ('a', 'ignored', 0) ON CONFLICT (key) DO NOTHING",
                &[],
            ),
            kvs,
        ),
        after(
            c(
                "upsert_where",
                "INSERT INTO kv (key, value, n) VALUES ('a', 'w', 5) ON CONFLICT (key) DO UPDATE SET n = EXCLUDED.n WHERE kv.n < EXCLUDED.n",
                COLL,
            ),
            kvs,
        ),
        c(
            "update_returning",
            "UPDATE users SET age = age + 1 WHERE city = 'Oslo' RETURNING id, age",
            &[],
        ),
        after(
            c(
                "update_subquery",
                "UPDATE orders SET status = 'vip' WHERE user_id IN (SELECT id FROM users WHERE city = 'Oslo')",
                &[],
            ),
            "SELECT id, status FROM orders ORDER BY id",
        ),
        after(
            c(
                "update_from",
                "UPDATE orders SET status = 'x' FROM users WHERE users.id = orders.user_id AND users.city = 'Bergen'",
                &[],
            ),
            "SELECT id, status FROM orders ORDER BY id",
        ),
        c(
            "delete_returning",
            "DELETE FROM orders WHERE status = 'open' RETURNING id",
            &[],
        ),
        after(
            c("delete_where", "DELETE FROM orders WHERE amount < 100", &[]),
            "SELECT id FROM orders ORDER BY id",
        ),
        // ---- functions with a stated tier
        c(
            "string_agg_single",
            "SELECT string_agg(name, ',') FROM users WHERE id = 1",
            &[],
        ),
        c("char_length", "SELECT char_length(name) FROM users", &[]),
        c(
            "agg_order_desc_nulls_last",
            "SELECT string_agg(name, '|' ORDER BY age DESC NULLS LAST, id) FROM users",
            COLL,
        ),
        c(
            "agg_order_default_nulls",
            "SELECT string_agg(name, '|' ORDER BY age, id) FROM users",
            COLL,
        ),
        exp(
            c(
                "refuse_agg_order_sum",
                "SELECT sum(amount ORDER BY id) FROM orders",
                COLL,
            ),
            Expect::Refused("fn:sum.order_by"),
        ),
        c(
            "btrim_forms",
            "SELECT btrim('  hi  '), btrim('xxhixx', 'x')",
            &[],
        ),
        c(
            "octet_length_utf8",
            "SELECT octet_length('abc'), octet_length('héllo')",
            &[],
        ),
        c(
            "now_as_text",
            "SELECT id FROM users WHERE now() IS NOT NULL AND id = 1",
            &[A::NowAsText],
        ),
        c(
            "cast_int_text",
            "SELECT CAST(id AS TEXT), CAST('12' AS INTEGER) FROM users WHERE id = 1",
            &[A::LenientCast],
        ),
        c(
            "lower_upper_ascii",
            "SELECT lower(name), upper(name) FROM users",
            &[A::AsciiOnlyCaseFolding],
        ),
        c(
            "int_division",
            "SELECT 7 / 2, -7 / 2, 7 % 3, -7 % 3 FROM users WHERE id = 1",
            &[A::DivisionByZeroIsNull],
        ),
        // ---- the allowance gate: without the allowance there is NO text
        exp(
            c("gate_now", "SELECT now()", &[]),
            Expect::NeedsAllowance(A::NowAsText),
        ),
        exp(
            c(
                "gate_like",
                "SELECT name FROM users WHERE name LIKE 'a%'",
                &[],
            ),
            Expect::NeedsAllowance(A::LikeCaseSensitivityIsConnectionSetting),
        ),
        exp(
            c("gate_division", "SELECT 1 / 2", &[]),
            Expect::NeedsAllowance(A::DivisionByZeroIsNull),
        ),
        exp(
            c(
                "gate_bool_literal",
                "SELECT id FROM users WHERE active = TRUE",
                &[],
            ),
            Expect::NeedsAllowance(A::BooleanAsInteger),
        ),
        // ---- the allowance is needed: engines genuinely disagree
        exp(
            c(
                "diverge_division_by_zero",
                "SELECT 1 / 0",
                &[A::DivisionByZeroIsNull],
            ),
            Expect::Diverges,
        ),
        exp(
            c(
                "diverge_bad_cast",
                "SELECT CAST('abc' AS INTEGER)",
                &[A::LenientCast],
            ),
            Expect::Diverges,
        ),
        // ---- refusals: no text
        ord(c(
            "distinct_on_emulated",
            "SELECT DISTINCT ON (city) city, id FROM users WHERE city IS NOT NULL ORDER BY city, id",
            COLL,
        )),
        exp(
            c(
                "refuse_for_update",
                "SELECT id FROM users WHERE id = 1 FOR UPDATE",
                &[],
            ),
            Expect::Refused("row_lock"),
        ),
        exp(
            c("refuse_unmapped_fn", "SELECT md5(name) FROM users", &[]),
            Expect::Refused("fn:md5"),
        ),
        exp(
            c(
                "refuse_cast_uuid",
                "SELECT CAST(name AS UUID) FROM users",
                &[A::LenientCast],
            ),
            Expect::Refused("cast.to_uuid"),
        ),
        // ---- reader: not in the IR (an error, never a pass-through)
        exp(
            c(
                "refuse_skip_locked",
                "SELECT id FROM users FOR UPDATE SKIP LOCKED",
                &[],
            ),
            Expect::Refused("row_lock"),
        ),
        exp(
            c(
                "refuse_for_share",
                "SELECT id FROM users WHERE id = 1 FOR SHARE",
                &[],
            ),
            Expect::Refused("row_lock.share"),
        ),
        exp(
            c("ir_json_path_op", "SELECT data #> '{o,k}' FROM docs", &[]),
            Expect::NotInIr("binary_operator"),
        ),
        exp(
            c(
                "ir_named_window",
                "SELECT id, sum(amount) OVER w FROM orders WINDOW w AS (ORDER BY id)",
                &[],
            ),
            Expect::NotInIr("select_clause"),
        ),
        // ---- array parameters: `= ANY($1)` binds as JSON text on SQLite (ParamBinding::JsonArrayText)
        p(
            c(
                "any_int_array",
                "SELECT id FROM users WHERE id = ANY($1)",
                ARR,
            ),
            vec![Param::IntArray(vec![1, 3, 5, 9])],
        ),
        p(
            c(
                "all_ne_int_array",
                "SELECT id FROM users WHERE id <> ALL($1)",
                ARR,
            ),
            vec![Param::IntArray(vec![1, 3])],
        ),
        p(
            c(
                "any_text_array",
                "SELECT id FROM users WHERE city = ANY($1)",
                ARR,
            ),
            vec![Param::TextArray(vec!["Oslo", "Bergen"])],
        ),
        p(
            c(
                "any_cast_hint",
                "SELECT id FROM users WHERE id = ANY($1::int[])",
                ARR,
            ),
            vec![Param::IntArray(vec![2, 4])],
        ),
        p(
            c(
                "any_empty_array",
                "SELECT id FROM users WHERE id = ANY($1)",
                ARR,
            ),
            vec![Param::IntArray(vec![])],
        ),
        p(
            c(
                "all_ne_empty_array",
                "SELECT id FROM users WHERE id <> ALL($1)",
                ARR,
            ),
            vec![Param::IntArray(vec![])],
        ),
        p(
            c(
                "any_with_scalar",
                "SELECT id FROM users WHERE id = ANY($1) AND age > $2",
                &[A::ArrayNullSemantics, A::TextCollation],
            ),
            vec![Param::IntArray(vec![1, 3, 5]), Param::Int(26)],
        ),
        c(
            "any_array_literal",
            "SELECT id FROM users WHERE id = ANY(ARRAY[1, 2])",
            &[],
        ),
        p(
            exp(
                c("gate_array", "SELECT id FROM users WHERE id = ANY($1)", &[]),
                Expect::NeedsAllowance(A::ArrayNullSemantics),
            ),
            vec![Param::IntArray(vec![1])],
        ),
        p(
            exp(
                c(
                    "diverge_null_array",
                    "SELECT id FROM users WHERE NOT (id = ANY($1))",
                    ARR,
                ),
                Expect::Diverges,
            ),
            vec![Param::Null],
        ),
        exp(
            c("ir_any_lt", "SELECT id FROM users WHERE id < ANY($1)", ARR),
            Expect::NotInIr("any_operator"),
        ),
        // ---- milestone B: DISTINCT ON emulation
        ord(c(
            "distinct_on_basic",
            "SELECT DISTINCT ON (city) city, id, name FROM users WHERE city IS NOT NULL ORDER BY city, age DESC, id",
            COLL,
        )),
        ord(c(
            "distinct_on_orders",
            "SELECT DISTINCT ON (user_id) user_id, amount, id FROM orders ORDER BY user_id, amount DESC, id",
            COLL,
        )),
        ord(c(
            "distinct_on_limit",
            "SELECT DISTINCT ON (user_id) user_id, id FROM orders ORDER BY user_id, id LIMIT 2",
            COLL,
        )),
        exp(
            c(
                "distinct_on_star_refused",
                "SELECT DISTINCT ON (city) * FROM users ORDER BY city, id",
                COLL,
            ),
            Expect::Refused("distinct_on.star_projection"),
        ),
        // ---- window functions, FILTER, frames
        ord(c(
            "win_row_number",
            "SELECT id, row_number() OVER (ORDER BY id) FROM users",
            COLL,
        )),
        ord(c(
            "win_rank_partition",
            "SELECT id, rank() OVER (PARTITION BY city ORDER BY age) FROM users ORDER BY id",
            COLL,
        )),
        ord(c(
            "win_running_sum",
            "SELECT id, sum(amount) OVER (PARTITION BY user_id ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM orders ORDER BY id",
            COLL,
        )),
        ord(c(
            "win_lag_lead",
            "SELECT id, lag(amount) OVER (ORDER BY id), lead(amount, 2) OVER (ORDER BY id) FROM orders ORDER BY id",
            COLL,
        )),
        c(
            "agg_filter",
            "SELECT count(*) FILTER (WHERE status = 'paid'), count(*) FROM orders",
            &[],
        ),
        // ---- WITH RECURSIVE
        ord(c(
            "with_recursive",
            "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 5) SELECT n FROM t ORDER BY n",
            COLL,
        )),
        // ---- TRIM syntax forms
        c(
            "trim_simple",
            "SELECT trim(name), trim('  x  ') FROM users WHERE id = 1",
            &[],
        ),
        c(
            "trim_both_chars",
            "SELECT trim(both 'x' from 'xxhixx')",
            &[],
        ),
        c(
            "trim_leading",
            "SELECT trim(leading 'x' from 'xxhixx')",
            &[],
        ),
        c(
            "trim_trailing",
            "SELECT trim(trailing 'x' from 'xxhixx')",
            &[],
        ),
        exp(
            c(
                "gap_trim_leading_from",
                "SELECT trim(leading from '  hi')",
                &[],
            ),
            Expect::ParserGap,
        ),
        exp(
            c(
                "gap_trim_trailing_from",
                "SELECT trim(trailing from 'hi  ')",
                &[],
            ),
            Expect::ParserGap,
        ),
        // ---- JSON operators
        ord(c(
            "json_get_text",
            "SELECT id, data->>'a' FROM docs ORDER BY id",
            &[A::JsonScalarAffinity, A::TextCollation],
        )),
        ord(c(
            "json_get_text_number",
            "SELECT id, data->>'n' FROM docs ORDER BY id",
            &[A::JsonScalarAffinity, A::TextCollation],
        )),
        ord(c(
            "json_nested",
            "SELECT id, data->'o'->>'k' FROM docs ORDER BY id",
            &[
                A::JsonScalarAffinity,
                A::JsonRepresentationAsText,
                A::TextCollation,
            ],
        )),
        ord(c(
            "json_cast_int",
            "SELECT id, CAST(data->>'n' AS INTEGER) + 1 FROM docs ORDER BY id",
            &[A::JsonScalarAffinity, A::LenientCast, A::TextCollation],
        )),
        exp(
            c(
                "diverge_json_boolean",
                "SELECT data->>'t' FROM docs WHERE id = 1",
                &[A::JsonScalarAffinity],
            ),
            Expect::Diverges,
        ),
        exp(
            c("gate_json", "SELECT data->>'a' FROM docs", &[]),
            Expect::NeedsAllowance(A::JsonScalarAffinity),
        ),
        // ---- make_interval arithmetic
        p(
            c(
                "interval_secs",
                "SELECT id FROM events WHERE at > now() - make_interval(secs => $1)",
                &[A::NowAsText, A::TimestampArithmeticAsText],
            ),
            vec![Param::Real(60.0)],
        ),
        p(
            c(
                "interval_wide",
                "SELECT id FROM events WHERE at > now() - make_interval(secs => $1)",
                &[A::NowAsText, A::TimestampArithmeticAsText],
            ),
            vec![Param::Real(1000.0)],
        ),
        p(
            c(
                "interval_mins",
                "SELECT id FROM events WHERE at > now() - make_interval(mins => $1)",
                &[A::NowAsText, A::TimestampArithmeticAsText],
            ),
            vec![Param::Int(5)],
        ),
        // ---- INTERVAL literals: read as the same make_interval form
        c(
            "interval_literal_minute",
            "SELECT id FROM events WHERE at > now() - interval '1 minute'",
            &[A::NowAsText, A::TimestampArithmeticAsText],
        ),
        c(
            "interval_literal_minutes",
            "SELECT id FROM events WHERE at > now() - INTERVAL '5 minutes'",
            &[A::NowAsText, A::TimestampArithmeticAsText],
        ),
        c(
            "interval_literal_hours",
            "SELECT id FROM events WHERE at > now() - interval '2 hours'",
            &[A::NowAsText, A::TimestampArithmeticAsText],
        ),
        c(
            "interval_literal_plus_days",
            "SELECT id FROM events WHERE at < now() + interval '7 days'",
            &[A::NowAsText, A::TimestampArithmeticAsText],
        ),
        exp(
            c(
                "refuse_interval_literal_month",
                "SELECT now() - interval '1 month'",
                &[A::NowAsText, A::TimestampArithmeticAsText],
            ),
            Expect::Refused("fn:make_interval.month_year"),
        ),
        exp(
            c(
                "ir_interval_compound",
                "SELECT now() - interval '1 day 2 hours'",
                &[A::NowAsText, A::TimestampArithmeticAsText],
            ),
            Expect::NotInIr("interval"),
        ),
        exp(
            c(
                "gate_interval",
                "SELECT id FROM events WHERE at > now() - make_interval(secs => 5)",
                &[A::NowAsText],
            ),
            Expect::NeedsAllowance(A::TimestampArithmeticAsText),
        ),
        exp(
            c(
                "refuse_interval_months",
                "SELECT now() - make_interval(months => 1)",
                &[A::NowAsText, A::TimestampArithmeticAsText],
            ),
            Expect::Refused("fn:make_interval.month_year"),
        ),
        exp(
            c(
                "refuse_interval_standalone",
                "SELECT make_interval(secs => 5)",
                &[],
            ),
            Expect::Refused("named_arg"),
        ),
        // ---- loud refusals named in the corpus
        exp(
            c("refuse_current_schema", "SELECT current_schema()", &[]),
            Expect::Refused("fn:current_schema"),
        ),
        exp(
            c(
                "refuse_to_regclass",
                "SELECT to_regclass('users') IS NOT NULL",
                &[],
            ),
            Expect::Refused("fn:to_regclass"),
        ),
        exp(
            c(
                "refuse_sha256",
                "SELECT encode(sha256(convert_to(name, 'UTF8')), 'hex') FROM users",
                &[],
            ),
            Expect::Refused("fn:convert_to"),
        ),
        exp(
            c(
                "refuse_regexp_replace",
                "SELECT regexp_replace(name, 'a', 'b') FROM users",
                &[],
            ),
            Expect::Refused("fn:regexp_replace"),
        ),
    ]
}

#[test]
fn postgres_to_sqlite_differential() {
    let mut e = Engines::open(PG, LITE)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    let (pgv, litev) = (e.pg_version(), e.sqlite_version());
    let cs = cases();
    let outcomes: Vec<_> = cs.iter().map(|c| run_case(&mut e, c)).collect();

    let mut log = String::new();
    writeln!(
        log,
        "postgres {pgv} -> sqlite {litev}; cases={}",
        outcomes.len()
    )
    .unwrap();
    for o in &outcomes {
        writeln!(
            log,
            "{} {:<28} {}",
            if o.passed { "PASS" } else { "FAIL" },
            o.name,
            o.detail.lines().next().unwrap_or("")
        )
        .unwrap();
        if let Some(t) = &o.translated {
            writeln!(log, "       -> {t}").unwrap();
        }
    }
    let failed: Vec<_> = outcomes.iter().filter(|o| !o.passed).collect();
    let matrix = unisql_differential::matrix_rows(&outcomes);
    writeln!(
        log,
        "\nMATRIX (construct, tier) from {} passing translated cases:",
        outcomes
            .iter()
            .filter(|o| o.passed && o.translated.is_some())
            .count()
    )
    .unwrap();
    for (k, t) in &matrix {
        writeln!(log, "  {k:<28} {t}").unwrap();
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let _ = std::fs::write(dir.join("DIFFERENTIAL-postgres-sqlite.txt"), &log);
    // The SQLite dialect module's matrix: how SQLite gives each construct, from PostgreSQL text.
    let v = litev.split_whitespace().next().unwrap_or("").to_string();
    let _ = std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../unisql-dialect-sqlite/matrix-sqlite-from-postgres.tsv"),
        unisql_differential::matrix_tsv(&outcomes, "sqlite", &v, "postgres", &[]),
    );
    println!("{log}");
    if !failed.is_empty() {
        for o in &failed {
            println!("---- FAILED {}\n{}", o.name, o.detail);
        }
        panic!(
            "{} of {} differential cases failed",
            failed.len(),
            outcomes.len()
        );
    }
}
