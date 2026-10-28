// SPDX-License-Identifier: MIT OR Apache-2.0
//! PostgreSQL -> MySQL differential suite (first MySQL slice).
//!
//! Needs PostgreSQL (`UNISQL_PG_URL`) and MySQL 8.4 (`UNISQL_MYSQL_URL`, default
//! `mysql://root:unisql@127.0.0.1:53306/uni`) started with `--character-set-server=utf8mb4
//! --collation-server=utf8mb4_bin`. That collation is the *premise* of the `text_collation`
//! allowance: it is what makes MySQL equality, ordering and LIKE agree with PostgreSQL's `C` locale.
//! Fails loudly if either engine is unreachable.
//!
//! Cases grant every allowance the translation may need; what each case actually *required* is in
//! the report and in the generated matrix. The allowance-gate and divergence cases are explicit.

use std::fmt::Write as _;

use unisql::Allowance as A;
use unisql_differential::{Case, Dir, Engines, Expect, KnownBad, Param, run_case_dir};

const PG: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active boolean NOT NULL, score double precision, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (key text PRIMARY KEY, value text, n integer);
CREATE TABLE docs (id integer PRIMARY KEY, data jsonb);
CREATE TABLE arrs (id integer PRIMARY KEY, data jsonb);
INSERT INTO arrs VALUES (1,'[10,"a",null,{"k":1},[2],true]'),(2,'{"a":1}'),(3,'"s"'),(4,'5'),(5,NULL),(6,'[]'),(7,'{"0":"zero"}'),(8,'null');
INSERT INTO docs VALUES (1,'{"a":"x","n":5,"t":true,"o":{"k":"deep"},"s":"null","f":1.5,"l":[1,2]}'),(2,'{"a":null,"n":7,"e":""}'),(3,'[1,2]'),(4,'"str"'),(5,NULL);
CREATE TABLE events (id integer PRIMARY KEY, at timestamptz NOT NULL);
CREATE TABLE fixed_ts (id integer PRIMARY KEY, at timestamptz NOT NULL);
CREATE TABLE edge_ts (id integer PRIMARY KEY, at timestamptz NOT NULL);
INSERT INTO edge_ts VALUES (1, '2020-12-31 08:30:00+00'), (2, '2021-01-01 08:30:00+00'), (3, '2021-01-04 08:30:00+00'), (4, '2018-12-31 08:30:00+00'), (5, '2016-01-03 08:30:00+00'), (6, '2024-12-30 08:30:00+00'), (7, '2027-01-01 08:30:00+00'), (8, '2019-06-30 08:30:00+00');
INSERT INTO fixed_ts VALUES (1, '2020-06-15 13:45:30+00'), (2, '1999-12-31 23:59:59+00'), (3, '2024-02-29 00:00:00+00');
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

const MYSQL: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name varchar(64) NOT NULL, email varchar(64), age integer,
                    active tinyint(1) NOT NULL, score double, city varchar(64));
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL, amount integer NOT NULL, status varchar(16) NOT NULL,
                     FOREIGN KEY (user_id) REFERENCES users(id));
CREATE TABLE kv (`key` varchar(64) PRIMARY KEY, value varchar(64), n integer);
CREATE TABLE docs (id integer PRIMARY KEY, data json);
CREATE TABLE arrs (id integer PRIMARY KEY, data json);
INSERT INTO arrs VALUES (1,'[10,"a",null,{"k":1},[2],true]'),(2,'{"a":1}'),(3,'"s"'),(4,'5'),(5,NULL),(6,'[]'),(7,'{"0":"zero"}'),(8,'null');
INSERT INTO docs VALUES (1,'{"a":"x","n":5,"t":true,"o":{"k":"deep"},"s":"null","f":1.5,"l":[1,2]}'),(2,'{"a":null,"n":7,"e":""}'),(3,'[1,2]'),(4,'"str"'),(5,NULL);
CREATE TABLE events (id integer PRIMARY KEY, at datetime NOT NULL);
CREATE TABLE fixed_ts (id integer PRIMARY KEY, at datetime NOT NULL);
CREATE TABLE edge_ts (id integer PRIMARY KEY, at datetime NOT NULL);
INSERT INTO edge_ts VALUES (1, '2020-12-31 08:30:00'), (2, '2021-01-01 08:30:00'), (3, '2021-01-04 08:30:00'), (4, '2018-12-31 08:30:00'), (5, '2016-01-03 08:30:00'), (6, '2024-12-30 08:30:00'), (7, '2027-01-01 08:30:00'), (8, '2019-06-30 08:30:00');
INSERT INTO fixed_ts VALUES (1, '2020-06-15 13:45:30'), (2, '1999-12-31 23:59:59'), (3, '2024-02-29 00:00:00');
INSERT INTO events VALUES (1, now() - interval 10 second),(2, now() - interval 120 second),(3, now() - interval 3600 second);
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

const LOCK_ONE: &str = "SELECT id FROM users WHERE id = 1 FOR UPDATE";

/// Everything a MySQL translation may legitimately need; each case's report says what it used.
const ALL: &[A] = &[
    A::TextCollation,
    A::LikeCaseSensitivityIsConnectionSetting,
    A::AsciiOnlyCaseFolding,
    A::LenientCast,
    A::DivisionByZeroIsNull,
    A::BooleanAsInteger,
    A::NowAsText,
    A::TimestampAsText,
    A::EpochWholeSeconds,
    A::TimestampWholeSeconds,
    A::TimestampArithmeticAsText,
    A::IntegerDivisionAssumed,
    A::UpsertAnyUniqueKey,
    A::AggregateStringTruncation,
    A::ArrayNullSemantics,
    A::RegexpFlavor,
    A::BitwiseNonNegativeIntegers,
    A::XorOperandsBoolean,
    A::UpsertWhereAsIf,
];

fn c(name: &'static str, sql: &'static str) -> Case {
    Case {
        name,
        sql,
        params: vec![],
        allow: ALL.to_vec(),
        ordered: false,
        after: None,
        hold: None,
        schema: None,
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
/// Another session holds `hold` (a locking SELECT, run on both engines) while the case runs.
fn held(mut x: Case, hold: &'static str) -> Case {
    x.hold = Some(hold);
    x
}
fn only(mut x: Case, allow: &[A]) -> Case {
    x.allow = allow.to_vec();
    x
}

fn cases(maria: bool) -> Vec<Case> {
    // `key` is a reserved word in MySQL, so the state queries avoid naming columns.
    let kvs_pg = "SELECT * FROM kv ORDER BY 1";
    let mut v = vec![
        // ---- placeholders: MySQL is positional, so param_order matters
        p(
            c("select_param", "SELECT id, name FROM users WHERE id = $1"),
            vec![Param::Int(3)],
        ),
        p(
            c(
                "param_reuse_reorder",
                "SELECT id FROM users WHERE id = $2 OR id = $1 OR id = $2",
            ),
            vec![Param::Int(1), Param::Int(3)],
        ),
        // ---- exact core
        c(
            "is_null",
            "SELECT id FROM users WHERE age IS NULL OR city IS NULL",
        ),
        c("is_true", "SELECT id FROM users WHERE (age > 30) IS TRUE"),
        c(
            "is_not_true",
            "SELECT id FROM users WHERE (age > 30) IS NOT TRUE",
        ),
        c("is_false", "SELECT id FROM users WHERE (age > 30) IS FALSE"),
        c(
            "is_not_false",
            "SELECT id FROM users WHERE (age > 30) IS NOT FALSE",
        ),
        c(
            "regexp_match",
            "SELECT id FROM users WHERE city ~ '^O.l' OR name !~ 'a$'",
        ),
        c(
            "bitwise_ops",
            "SELECT id, id & 3, id | 4, id # 1, id << 1, id >> 1 FROM users ORDER BY id",
        ),
        c(
            "ceil_floor",
            "SELECT id, ceil(score), ceiling(score), floor(score), floor(-score) FROM users",
        ),
        c(
            "mod_function",
            "SELECT id, mod(age, 7), mod(-age, 7) FROM users",
        ),
        c(
            "string_functions",
            "SELECT id, reverse(name), repeat(name, 2), repeat(name, 0), repeat(name, -1), reverse(email), concat_ws('-', name, city, email), concat_ws(NULL, name, city), concat_ws('', name), reverse('h\u{e9}llo'), repeat(email, id) FROM users",
        ),
        c(
            "left_right",
            "SELECT id, left(name, 2), right(name, 2), left(name, 0), right(name, 0), left(name, 100), right(name, 100), left(email, 3), left('h\u{e9}llo', 2), right('h\u{e9}llo', 2) FROM users",
        ),
        c(
            "lpad_rpad",
            "SELECT id, lpad(name, 8, '*'), rpad(name, 8, 'ab'), lpad(name, 3, '*'), rpad(name, 0, 'x'), lpad(city, 6, 'xy'), lpad('\u{e9}', 3, '\u{fc}'), rpad(email, 4, '.') FROM users",
        ),
        exp(
            c("refuse_left_column", "SELECT left(name, age) FROM users"),
            Expect::Refused("fn:left.length"),
        ),
        exp(
            c("refuse_right_negative", "SELECT right(name, -1) FROM users"),
            Expect::Refused("fn:right.length"),
        ),
        exp(
            c(
                "refuse_lpad_empty_pad",
                "SELECT lpad(name, 5, '') FROM users",
            ),
            Expect::Refused("fn:lpad.args"),
        ),
        exp(
            c(
                "refuse_date_trunc_week_param",
                "SELECT date_trunc('week', $1::timestamp)",
            ),
            Expect::Refused("fn:date_trunc.parameter"),
        ),
        ord(c(
            "json_get_text",
            "SELECT id, data->>'a', data->>'n', data->>'t', data->>'o', data->>'s', data->>'f', data->>'l', data->>'e', data->>'zz' FROM docs ORDER BY id",
        )),
        ord(c(
            "json_get",
            "SELECT id, (data->'a')::text, (data->'n')::text, (data->'t')::text, (data->'o')::text, (data->'s')::text, (data->'f')::text, (data->'l')::text, (data->'e')::text, (data->'zz')::text FROM docs ORDER BY id",
        )),
        ord(c(
            "json_nested",
            "SELECT id, data->'o'->>'k', (data->'o'->'k')::text, data->'l'->>'x' FROM docs ORDER BY id",
        )),
        ord(c(
            "json_in_where",
            "SELECT id FROM docs WHERE data->>'a' = 'x' OR data->>'n' = '7' ORDER BY id",
        )),
        ord(c(
            "json_index",
            "SELECT id, (data->0)::text, (data->1)::text, (data->2)::text, (data->3)::text, (data->4)::text, (data->5)::text, (data->6)::text, (data-> -1)::text, (data-> -2)::text, (data-> -6)::text, (data-> -7)::text FROM arrs ORDER BY id",
        )),
        ord(c(
            "json_index_text",
            "SELECT id, data->>0, data->>1, data->>2, data->>3, data->>4, data->>5, data->>6, data->> -1, data->> -2, data->> -6, data->> -7 FROM arrs ORDER BY id",
        )),
        ord(c(
            "json_index_nested",
            "SELECT id, data->3->>'k', data->4->>0, (data->0)::text = '10' FROM arrs ORDER BY id",
        )),
        exp(
            c("refuse_json_index_param", "SELECT $1->0"),
            Expect::Refused("json_get.parameter"),
        ),
        exp(
            c("refuse_json_index_text_param", "SELECT $1->>0"),
            Expect::Refused("json_get_text.parameter"),
        ),
        exp(
            c("refuse_json_index_expr", "SELECT data->(1+1) FROM arrs"),
            Expect::Refused("json_get.key"),
        ),
        exp(
            c("refuse_json_key_param", "SELECT data->>$1 FROM docs"),
            Expect::Refused("json_get_text.key"),
        ),
        exp(
            c("refuse_json_doc_param", "SELECT $1->>'a'"),
            Expect::Refused("json_get_text.parameter"),
        ),
        c(
            "split_part_literal",
            "SELECT id, split_part(email, '@', 1), split_part(email, '@', 2), split_part(email, '@', 3), split_part(name, 'a', 2), split_part(name, 'zz', 1), split_part(city, 'ab', 4), split_part('a,b,,d', ',', 3), split_part('a,b,,d', ',', 5), split_part('', ',', 1), split_part('h\u{e9}llo', '\u{e9}', 2), split_part('aaa', 'aa', 2) FROM users",
        ),
        exp(
            c(
                "refuse_split_part_zero",
                "SELECT split_part(email, '@', 0) FROM users",
            ),
            Expect::Refused("fn:split_part.args"),
        ),
        exp(
            c(
                "refuse_split_part_param",
                "SELECT split_part($1::text, ',', 2)",
            ),
            Expect::Refused("fn:split_part.parameter"),
        ),
        c(
            "strpos_position",
            "SELECT id, strpos(name, 'a'), position('a' in name), strpos(name, 'zzz'), strpos(name, '') FROM users",
        ),
        c(
            "in_list",
            "SELECT id FROM users WHERE id IN (1, 3, 5) AND id NOT IN (5)",
        ),
        c(
            "between",
            "SELECT id FROM users WHERE age BETWEEN 25 AND 41",
        ),
        c(
            "comparison_ops",
            "SELECT id FROM users WHERE age >= 30 AND age < 60 AND id <> 3",
        ),
        c("text_equals", "SELECT id FROM users WHERE city = 'Oslo'"),
        c(
            "arith",
            "SELECT id, age * 2 + 1 FROM users WHERE age IS NOT NULL",
        ),
        c(
            "case_when",
            "SELECT id, CASE WHEN age >= 40 THEN 'senior' WHEN age IS NULL THEN 'unknown' ELSE 'adult' END FROM users",
        ),
        c(
            "coalesce_nullif",
            "SELECT id, coalesce(city, 'none'), nullif(age, 25) FROM users",
        ),
        c(
            "string_funcs",
            "SELECT id, length(name), replace(name, 'a', 'A'), ltrim('  x') FROM users",
        ),
        c(
            "concat_op",
            "SELECT name || '-' || coalesce(city, '?') FROM users",
        ),
        c(
            "aggregates",
            "SELECT count(*), count(status), sum(amount), min(amount), max(amount) FROM orders",
        ),
        c(
            "count_distinct",
            "SELECT count(DISTINCT user_id) FROM orders",
        ),
        c(
            "avg_cast",
            "SELECT CAST(avg(amount) AS DOUBLE PRECISION) FROM orders",
        ),
        c(
            "group_having",
            "SELECT user_id, count(*) AS n, sum(amount) AS total FROM orders GROUP BY user_id HAVING sum(amount) > 100",
        ),
        c("distinct", "SELECT DISTINCT city FROM users"),
        c(
            "inner_join",
            "SELECT u.name, o.amount FROM users u JOIN orders o ON o.user_id = u.id WHERE o.status = 'paid'",
        ),
        c(
            "left_join",
            "SELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id",
        ),
        c(
            "in_subquery",
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE amount > 100)",
        ),
        c(
            "exists",
            "SELECT name FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND o.status = 'open')",
        ),
        c(
            "scalar_subquery",
            "SELECT id, (SELECT count(*) FROM orders o WHERE o.user_id = u.id) FROM users u",
        ),
        c(
            "union_all",
            "SELECT id FROM users WHERE id < 3 UNION ALL SELECT id FROM orders WHERE id < 3",
        ),
        c(
            "union_distinct",
            "SELECT user_id FROM orders UNION SELECT id FROM users WHERE id > 4",
        ),
        c(
            "intersect",
            "SELECT id FROM users INTERSECT SELECT user_id FROM orders",
        ),
        c(
            "except",
            "SELECT id FROM users EXCEPT SELECT user_id FROM orders",
        ),
        c(
            "cte",
            "WITH big AS (SELECT user_id, sum(amount) AS total FROM orders GROUP BY user_id) SELECT u.name, big.total FROM users u JOIN big ON big.user_id = u.id",
        ),
        ord(c(
            "with_recursive",
            "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 5) SELECT n FROM t ORDER BY n",
        )),
        // ---- ordering and NULL placement (MySQL has no NULLS FIRST/LAST)
        ord(c(
            "order_asc_nulls",
            "SELECT id, age FROM users ORDER BY age, id",
        )),
        ord(c(
            "order_desc_nulls",
            "SELECT id, age FROM users ORDER BY age DESC, id",
        )),
        ord(c(
            "order_nulls_first",
            "SELECT id, age FROM users ORDER BY age NULLS FIRST, id",
        )),
        ord(c(
            "order_desc_nulls_last",
            "SELECT id, age FROM users ORDER BY age DESC NULLS LAST, id",
        )),
        ord(c(
            "limit_offset",
            "SELECT id FROM users ORDER BY id LIMIT 2 OFFSET 1",
        )),
        ord(c(
            "offset_only",
            "SELECT id FROM users ORDER BY id OFFSET 4",
        )),
        ord(c("order_text", "SELECT name FROM users ORDER BY name")),
        // ---- row locks. Another session holds users.id = 1; what the statement does to that row is
        // measured on both engines (NOWAIT raises, SKIP LOCKED leaves it out, a free row is fine).
        exp(
            held(
                c(
                    "lock_nowait_on_locked_row",
                    "SELECT id FROM users WHERE id = 1 FOR UPDATE NOWAIT",
                ),
                LOCK_ONE,
            ),
            Expect::BothError,
        ),
        held(
            c(
                "lock_nowait_on_free_row",
                "SELECT id FROM users WHERE id = 2 FOR UPDATE NOWAIT",
            ),
            LOCK_ONE,
        ),
        ord(held(
            c(
                "lock_skip_locked",
                "SELECT id FROM users WHERE id IN (1, 2, 3) ORDER BY id FOR UPDATE SKIP LOCKED",
            ),
            LOCK_ONE,
        )),
        // The queue-worker pattern: take the first row nobody else holds.
        ord(held(
            c(
                "lock_skip_locked_limit",
                "SELECT id FROM users ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED",
            ),
            LOCK_ONE,
        )),
        c(
            "lock_for_update_plain",
            "SELECT id FROM users WHERE id = 4 FOR UPDATE",
        ),
        // ---- LIKE family
        p(
            c("like_param", "SELECT name FROM users WHERE name LIKE $1"),
            vec![Param::Text("a%")],
        ),
        c(
            "like_case_sensitive",
            "SELECT name FROM users WHERE name LIKE 'bob'",
        ),
        c(
            "like_escape_percent",
            "SELECT name FROM users WHERE name LIKE 'frank\\%'",
        ),
        c("not_like", "SELECT id FROM users WHERE name NOT LIKE '%a%'"),
        p(
            c("ilike", "SELECT id FROM users WHERE email ILIKE $1"),
            vec![Param::Text("%EXAMPLE.COM")],
        ),
        // PostgreSQL's ILIKE in the C locale folds ASCII only; MySQL's LOWER() folds Unicode. The
        // divergence is what the `ascii_only_case_folding` allowance on `ilike` stands for.
        exp(
            c("ilike_non_ascii", "SELECT 'É' ILIKE 'é'"),
            Expect::Diverges,
        ),
        // ---- DML
        after(
            p(
                c(
                    "insert_values",
                    "INSERT INTO users (id, name, active) VALUES ($1, $2, TRUE)",
                ),
                vec![Param::Int(7), Param::Text("gus")],
            ),
            "SELECT id, name, active FROM users WHERE id = 7",
        ),
        after(
            p(
                c(
                    "upsert_do_update",
                    "INSERT INTO kv (key, value, n) VALUES ($1, $2, $3) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, n = EXCLUDED.n",
                ),
                vec![Param::Text("a"), Param::Text("new"), Param::Int(11)],
            ),
            kvs_pg,
        ),
        // the WHERE column is set first in the source; the target must assign it last
        after(
            c(
                "upsert_where_updates",
                "INSERT INTO kv (key, value, n) VALUES ('a', 'w', 5) ON CONFLICT (key) DO UPDATE SET n = EXCLUDED.n, value = EXCLUDED.value WHERE kv.n < EXCLUDED.n",
            ),
            kvs_pg,
        ),
        after(
            c(
                "upsert_where_kept",
                "INSERT INTO kv (key, value, n) VALUES ('b', 'w', 1) ON CONFLICT (key) DO UPDATE SET n = EXCLUDED.n, value = EXCLUDED.value WHERE kv.n < EXCLUDED.n",
            ),
            kvs_pg,
        ),
        after(
            c(
                "upsert_where_null_kept",
                "INSERT INTO kv (key, value, n) VALUES ('c', 'w', 9) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, n = EXCLUDED.n WHERE kv.n < EXCLUDED.n",
            ),
            kvs_pg,
        ),
        after(
            c(
                "upsert_where_inserts",
                "INSERT INTO kv (key, value, n) VALUES ('new', 'w', 1) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, n = EXCLUDED.n WHERE kv.n < EXCLUDED.n",
            ),
            kvs_pg,
        ),
        after(
            p(
                c(
                    "upsert_where_param",
                    "INSERT INTO kv (key, value, n) VALUES ($1, $2, $3) ON CONFLICT (key) DO UPDATE SET n = EXCLUDED.n, value = EXCLUDED.value WHERE kv.n < $3",
                ),
                vec![Param::Text("a"), Param::Text("p"), Param::Int(7)],
            ),
            kvs_pg,
        ),
        exp(
            only(
                c(
                    "gate_upsert_where",
                    "INSERT INTO kv (key, value, n) VALUES ('a', 'w', 5) ON CONFLICT (key) DO UPDATE SET n = EXCLUDED.n WHERE kv.n < EXCLUDED.n",
                ),
                &[A::UpsertAnyUniqueKey],
            ),
            Expect::NeedsAllowance(A::UpsertWhereAsIf),
        ),
        after(
            c(
                "upsert_do_nothing",
                "INSERT INTO kv (key, value, n) VALUES ('a', 'ignored', 0) ON CONFLICT (key) DO NOTHING",
            ),
            kvs_pg,
        ),
        after(
            c(
                "update_subquery",
                "UPDATE orders SET status = 'vip' WHERE user_id IN (SELECT id FROM users WHERE city = 'Oslo')",
            ),
            "SELECT id, status FROM orders ORDER BY id",
        ),
        after(
            c("delete_where", "DELETE FROM orders WHERE amount < 100"),
            "SELECT id FROM orders ORDER BY id",
        ),
        // ---- windows, DISTINCT ON via row_number, TRIM
        ord(c(
            "win_row_number",
            "SELECT id, row_number() OVER (ORDER BY id) FROM users",
        )),
        ord(c(
            "win_running_sum",
            "SELECT id, sum(amount) OVER (PARTITION BY user_id ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM orders ORDER BY id",
        )),
        ord(c(
            "distinct_on_basic",
            "SELECT DISTINCT ON (city) city, id, name FROM users WHERE city IS NOT NULL ORDER BY city, age DESC, id",
        )),
        c("trim_both_char", "SELECT trim(both 'x' from 'xxhixx')"),
        c(
            "trim_leading_char",
            "SELECT trim(leading 'x' from 'xxhixx')",
        ),
        c("btrim_default", "SELECT btrim('  hi  ')"),
        c(
            "string_agg_single",
            "SELECT string_agg(name, ',') FROM users WHERE id = 1",
        ),
        c(
            "agg_order_desc_nulls_last",
            "SELECT string_agg(name, '|' ORDER BY age DESC NULLS LAST, id) FROM users",
        ),
        c(
            "agg_order_default_nulls",
            "SELECT string_agg(name, '|' ORDER BY age, id) FROM users",
        ),
        c(
            "agg_order_nulls_first",
            "SELECT string_agg(name, '|' ORDER BY age NULLS FIRST, id) FROM users",
        ),
        exp(
            c(
                "refuse_agg_order_sum",
                "SELECT sum(amount ORDER BY id) FROM orders",
            ),
            Expect::Refused("fn:sum.order_by"),
        ),
        c(
            "now_not_null",
            "SELECT id FROM users WHERE now() IS NOT NULL AND id = 1",
        ),
        c(
            "int_division",
            "SELECT 7 / 2, -7 / 2, 7 % 3, -7 % 3 FROM users WHERE id = 1",
        ),
        c(
            "cast_int_text",
            "SELECT CAST(id AS TEXT), CAST('12' AS INTEGER) FROM users WHERE id = 1",
        ),
        p(
            c(
                "interval_secs",
                "SELECT id FROM events WHERE at > now() - make_interval(secs => $1)",
            ),
            vec![Param::Real(60.0)],
        ),
        p(
            c(
                "interval_wide",
                "SELECT id FROM events WHERE at > now() - make_interval(secs => $1)",
            ),
            vec![Param::Real(1000.0)],
        ),
        exp(
            c(
                "refuse_interval_positional",
                "SELECT make_interval(0, 0, 0, 1)",
            ),
            Expect::Refused("fn:make_interval.standalone"),
        ),
        c(
            "current_date_text",
            "SELECT CAST(CURRENT_DATE AS TEXT) FROM users WHERE id = 1",
        ),
        // ---- INTERVAL literals: read as the same make_interval form
        c(
            "interval_literal_minute",
            "SELECT id FROM events WHERE at > now() - interval '1 minute'",
        ),
        c(
            "interval_literal_minutes",
            "SELECT id FROM events WHERE at > now() - INTERVAL '5 minutes'",
        ),
        c(
            "interval_literal_hours",
            "SELECT id FROM events WHERE at > now() - interval '2 hours'",
        ),
        c(
            "interval_literal_plus_days",
            "SELECT id FROM events WHERE at < now() + interval '7 days'",
        ),
        exp(
            c(
                "refuse_interval_literal_month",
                "SELECT now() - interval '1 month'",
            ),
            Expect::Refused("fn:make_interval.month_year"),
        ),
        exp(
            c(
                "ir_interval_compound",
                "SELECT now() - interval '1 day 2 hours'",
            ),
            Expect::NotInIr("interval"),
        ),
        // ---- array parameters bind as JSON text (ParamBinding::JsonArrayText) via JSON_CONTAINS
        p(
            c("any_int_array", "SELECT id FROM users WHERE id = ANY($1)"),
            vec![Param::IntArray(vec![1, 3, 5, 9])],
        ),
        p(
            c(
                "all_ne_int_array",
                "SELECT id FROM users WHERE id <> ALL($1)",
            ),
            vec![Param::IntArray(vec![1, 3])],
        ),
        p(
            c(
                "any_text_array",
                "SELECT id FROM users WHERE city = ANY($1)",
            ),
            vec![Param::TextArray(vec!["Oslo", "Bergen"])],
        ),
        p(
            c("any_empty_array", "SELECT id FROM users WHERE id = ANY($1)"),
            vec![Param::IntArray(vec![])],
        ),
        p(
            c(
                "all_ne_empty_array",
                "SELECT id FROM users WHERE id <> ALL($1)",
            ),
            vec![Param::IntArray(vec![])],
        ),
        p(
            c(
                "any_with_scalar_reorder",
                "SELECT id FROM users WHERE age > $2 AND id = ANY($1) AND age < $2 + 100",
            ),
            vec![Param::IntArray(vec![1, 3, 5]), Param::Int(26)],
        ),
        c(
            "any_array_literal",
            "SELECT id FROM users WHERE id = ANY(ARRAY[1, 2])",
        ),
        p(
            exp(
                only(
                    c("gate_array", "SELECT id FROM users WHERE id = ANY($1)"),
                    &[],
                ),
                Expect::NeedsAllowance(A::ArrayNullSemantics),
            ),
            vec![Param::IntArray(vec![1])],
        ),
        // ---- the allowance gate: without it, no text
        exp(
            only(
                c(
                    "gate_text_collation",
                    "SELECT id FROM users WHERE city = 'Oslo'",
                ),
                &[],
            ),
            Expect::NeedsAllowance(A::TextCollation),
        ),
        exp(
            only(
                c("gate_division", "SELECT 7 / 2"),
                &[A::DivisionByZeroIsNull],
            ),
            Expect::NeedsAllowance(A::IntegerDivisionAssumed),
        ),
        exp(
            only(
                c(
                    "gate_upsert_key",
                    "INSERT INTO kv (key, value) VALUES ('a','b') ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                ),
                &[A::TextCollation],
            ),
            Expect::NeedsAllowance(A::UpsertAnyUniqueKey),
        ),
        // ---- engines disagree: the allowance exists for a reason
        exp(
            c("diverge_division_by_zero", "SELECT 1 / 0"),
            Expect::Diverges,
        ),
        exp(
            c("diverge_bad_cast", "SELECT CAST('abc' AS INTEGER)"),
            Expect::Diverges,
        ),
        // ---- refusals
        exp(
            c(
                "refuse_upsert_where_two_columns",
                "INSERT INTO kv (key, value, n) VALUES ('a','w',5) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, n = EXCLUDED.n WHERE kv.n < EXCLUDED.n AND kv.value IS NOT NULL",
            ),
            Expect::Refused("upsert_where.two_columns"),
        ),
        exp(
            c(
                "refuse_filter",
                "SELECT count(*) FILTER (WHERE status = 'paid') FROM orders",
            ),
            Expect::Refused("filter_clause"),
        ),
        exp(
            c("refuse_json", "SELECT id, data->>city FROM docs, users"),
            Expect::Refused("json_get_text.key"),
        ),
        exp(
            c("refuse_substr", "SELECT substr(name, 0, 3) FROM users"),
            Expect::Refused("fn:substr"),
        ),
        exp(
            c("refuse_concat_fn", "SELECT concat(name, city) FROM users"),
            Expect::Refused("fn:concat"),
        ),
        exp(
            c("refuse_trim_set", "SELECT trim(both 'xy' from 'xyxhi')"),
            Expect::Refused("trim.multi_char"),
        ),
        after(
            c(
                "update_from",
                "UPDATE orders SET status = 'x' FROM users WHERE users.id = orders.user_id AND users.city = 'Bergen'",
            ),
            "SELECT id, status FROM orders ORDER BY id",
        ),
        after(
            c(
                "delete_using",
                "DELETE FROM orders USING users WHERE users.id = orders.user_id AND users.city = 'Bergen'",
            ),
            "SELECT id FROM orders ORDER BY id",
        ),
        exp(
            c("refuse_cast_uuid", "SELECT CAST(name AS UUID) FROM users"),
            Expect::Refused("cast.to_uuid"),
        ),
        exp(
            c(
                "refuse_cast_regclass",
                "SELECT CAST(name AS REGCLASS) FROM users",
            ),
            Expect::Refused("cast.to_regclass"),
        ),
    ];
    if maria {
        // MariaDB >= 10.5: INSERT/DELETE .. RETURNING; UPDATE .. RETURNING is still refused.
        v.push(c(
            "insert_returning",
            "INSERT INTO kv (key, value, n) VALUES ('d', 'w', 9) RETURNING key, n",
        ));
        v.push(c(
            "delete_returning",
            "DELETE FROM orders WHERE status = 'open' RETURNING id",
        ));
        // MariaDB has no UPDATE .. RETURNING: it must be refused, not sent to the engine.
        v.push(exp(
            c(
                "refuse_update_returning",
                "UPDATE users SET age = age + 1 WHERE city = 'Oslo' RETURNING id, age",
            ),
            Expect::Refused("returning.update"),
        ));
    } else {
        v.push(exp(
            c(
                "refuse_returning",
                "INSERT INTO kv (key, value, n) VALUES ('d', 'w', 9) RETURNING key, n",
            ),
            Expect::Refused("returning"),
        ));
    }
    v.push(exp(
        c(
            "refuse_data_modifying_cte",
            "WITH s AS (DELETE FROM orders WHERE amount = 0 RETURNING id) SELECT id FROM s",
        ),
        Expect::Refused("cte.data_modifying"),
    ));
    // WITH before a modifying statement: MySQL has UPDATE and DELETE (not INSERT), MariaDB none.
    for (name, sql, leaf, mysql_ok) in [
        (
            "with_update",
            "WITH s AS (SELECT key FROM kv WHERE n = 1) UPDATE kv SET n = n + 10 WHERE key IN (SELECT key FROM s)",
            "with.update",
            true,
        ),
        (
            "with_delete",
            "WITH s AS (SELECT key FROM kv WHERE n = 1) DELETE FROM kv WHERE key IN (SELECT key FROM s)",
            "with.delete",
            true,
        ),
        (
            "with_insert",
            "WITH s AS (SELECT 'z' AS key) INSERT INTO kv (key, value, n) SELECT key, 'v', 9 FROM s",
            "with.insert",
            false,
        ),
    ] {
        if mysql_ok && !maria {
            v.push(Case {
                after: Some("SELECT * FROM kv ORDER BY 1"),
                ..c(name, sql)
            });
        } else {
            v.push(exp(c(name, sql), Expect::Refused(leaf)));
        }
    }
    v.push(c(
        "escape_string_literal",
        "SELECT char_length(E'a\\nb\\t\\\\') FROM users WHERE id = 1",
    ));
    // LATERAL: MySQL 8.0.14+ has it; MariaDB does not.
    if maria {
        v.push(exp(
            c("refuse_left_join_lateral", "SELECT u.id, top.amount FROM users u LEFT JOIN LATERAL (SELECT o.amount FROM orders o WHERE o.user_id = u.id ORDER BY o.amount DESC LIMIT 1) top ON true ORDER BY u.id"),
            Expect::Refused("lateral"),
        ));
    } else {
        v.push(ord(c("left_join_lateral", "SELECT u.id, top.amount FROM users u LEFT JOIN LATERAL (SELECT o.amount FROM orders o WHERE o.user_id = u.id ORDER BY o.amount DESC LIMIT 1) top ON true ORDER BY u.id")));
    }
    v.push(exp(
        c(
            "refuse_at_time_zone",
            "SELECT CAST(CAST('2020-01-01 00:00:00+00' AS TIMESTAMPTZ) AT TIME ZONE 'UTC' AS TEXT)",
        ),
        Expect::Refused("at_time_zone"),
    ));
    v.push(exp(
        c(
            "refuse_array_subquery",
            "SELECT array_length(ARRAY(SELECT id FROM users), 1)",
        ),
        Expect::Refused("array_subquery"),
    ));
    // A Schema removes the text-collation assumption from comparisons on non-text columns.
    {
        use unisql::Column;
        let users = unisql::Schema::new()
            .with_column("users", "id", Column::numeric())
            .with_column("users", "name", Column::text())
            .with_column("users", "age", Column::numeric());
        v.push(Case {
            schema: Some(users.clone()),
            params: vec![Param::Int(25)],
            allow: vec![],
            ..c(
                "schema_numeric_column_needs_no_allowance",
                "SELECT id FROM users WHERE age > $1 AND age BETWEEN 20 AND 60",
            )
        });
        v.push(exp(
            Case {
                schema: Some(users),
                params: vec![Param::Text("b")],
                allow: vec![],
                ..c(
                    "schema_text_column_still_conditional",
                    "SELECT id FROM users WHERE name > $1",
                )
            },
            Expect::NeedsAllowance(A::TextCollation),
        ));
        v.push(exp(
            Case {
                params: vec![Param::Int(25)],
                allow: vec![],
                ..c(
                    "no_schema_unknown_column_is_conditional",
                    "SELECT id FROM users WHERE age > $1",
                )
            },
            Expect::NeedsAllowance(A::TextCollation),
        ));
    }
    for (name, sql, leaf) in [
        (
            "refuse_extract_century",
            "SELECT EXTRACT(CENTURY FROM now())",
            "extract.century",
        ),
        (
            "refuse_array_literal",
            "SELECT array_length(ARRAY[1, 2, 3], 1)",
            "array_literal",
        ),
        (
            "refuse_current_user",
            "SELECT CAST(current_user AS TEXT)",
            "fn:current_user",
        ),
    ] {
        v.push(exp(c(name, sql), Expect::Refused(leaf)));
    }
    // ---- verified mappings (fixed timestamps; fields compared with PostgreSQL's on the engine)
    v.push(ord(c(
        "extract_fields",
        "SELECT id, CAST(EXTRACT(YEAR FROM at) AS INTEGER), CAST(EXTRACT(MONTH FROM at) AS INTEGER), CAST(EXTRACT(DAY FROM at) AS INTEGER), CAST(EXTRACT(HOUR FROM at) AS INTEGER), CAST(EXTRACT(MINUTE FROM at) AS INTEGER) FROM fixed_ts ORDER BY id",
    )));
    v.push(ord(c(
        "extract_calendar_fields",
        "SELECT id, CAST(EXTRACT(DOW FROM at) AS INTEGER), CAST(EXTRACT(ISODOW FROM at) AS INTEGER), CAST(EXTRACT(DOY FROM at) AS INTEGER), CAST(EXTRACT(QUARTER FROM at) AS INTEGER), CAST(EXTRACT(WEEK FROM at) AS INTEGER) FROM edge_ts ORDER BY id",
    )));
    v.push(ord(c(
        "extract_epoch",
        "SELECT id, CAST(EXTRACT(EPOCH FROM at) AS BIGINT) FROM fixed_ts ORDER BY id",
    )));
    v.push(c("to_timestamp_epoch", "SELECT CAST(EXTRACT(YEAR FROM to_timestamp(1592228730)) AS INTEGER), CAST(EXTRACT(HOUR FROM to_timestamp(1592228730)) AS INTEGER), CAST(EXTRACT(EPOCH FROM to_timestamp(1592228730)) AS BIGINT)"));
    v.push(ord(c("date_trunc_fields", "SELECT id, CAST(EXTRACT(MINUTE FROM date_trunc('hour', at)) AS INTEGER), CAST(EXTRACT(HOUR FROM date_trunc('day', at)) AS INTEGER), CAST(EXTRACT(DAY FROM date_trunc('month', at)) AS INTEGER), CAST(EXTRACT(MONTH FROM date_trunc('year', at)) AS INTEGER), CAST(EXTRACT(EPOCH FROM date_trunc('minute', at)) AS BIGINT) FROM fixed_ts ORDER BY id")));
    v.push(c("epoch_ms_of_clock", "SELECT CAST(EXTRACT(EPOCH FROM clock_timestamp()) * 1000 AS BIGINT) > 1700000000000 AND CAST(EXTRACT(EPOCH FROM clock_timestamp()) * 1000 AS BIGINT) < 4102444800000"));
    v.push(ord(c("epoch_of_difference", "SELECT a.id, b.id, CAST(EXTRACT(EPOCH FROM (a.at - b.at)) AS BIGINT) FROM fixed_ts a JOIN fixed_ts b ON a.id < b.id ORDER BY a.id, b.id")));
    v.push(exp(
        c("refuse_age", "SELECT age(now(), now())"),
        Expect::Refused("fn:age"),
    ));
    v.push(ord(c("to_char_fields", "SELECT id, to_char(at, 'YYYY-MM-DD HH24:MI:SS'), to_char(at, 'DD/MM/YYYY'), to_char(at, 'HH24:MI') FROM edge_ts ORDER BY id")));
    v.push(exp(
        c("refuse_to_char_day_name", "SELECT to_char(now(), 'Day')"),
        Expect::Refused("fn:to_char.format"),
    ));
    v.push(ord(c("date_part_fields", "SELECT id, CAST(date_part('year', at) AS INTEGER), CAST(date_part('dow', at) AS INTEGER), CAST(date_part('doy', at) AS INTEGER), CAST(date_part('week', at) AS INTEGER) FROM edge_ts ORDER BY id")));
    v.push(exp(
        c(
            "refuse_date_part_dynamic",
            "SELECT date_part(city, now()) FROM users",
        ),
        Expect::Refused("fn:date_part"),
    ));
    v.push(ord(c("date_trunc_week_quarter", "SELECT id, CAST(EXTRACT(EPOCH FROM date_trunc('week', at)) AS BIGINT), CAST(EXTRACT(EPOCH FROM date_trunc('quarter', at)) AS BIGINT) FROM edge_ts ORDER BY id")));
    v.push(exp(
        c(
            "refuse_date_trunc_decade",
            "SELECT date_trunc('decade', now())",
        ),
        Expect::Refused("fn:date_trunc.decade"),
    ));
    v.push(exp(
        c(
            "gen_random_uuid_v4",
            "SELECT CAST(gen_random_uuid() AS TEXT) FROM users",
        ),
        Expect::RandomUuidV4,
    ));
    v.push(ord(c(
        "table_function_series_up",
        "SELECT g FROM generate_series(1, 5) AS g ORDER BY g",
    )));
    v.push(ord(c(
        "table_function_series_down_step",
        "SELECT g FROM generate_series(10, -4, -3) AS g ORDER BY g",
    )));
    v.push(ord(c(
        "table_function_series_empty",
        "SELECT count(*) AS n FROM generate_series(5, 1) AS g",
    )));
    v.push(ord(c(
        "table_function_series_joined",
        "SELECT u.id, g FROM users u JOIN generate_series(1, 2) AS g ON g = u.id ORDER BY u.id",
    )));
    v.push(exp(
        c(
            "table_function_series_args_refused",
            "SELECT g FROM generate_series(1, 2 + 1) AS g",
        ),
        Expect::Refused("table_function.generate_series.args"),
    ));
    v.push(exp(
        c(
            "table_function_series_alias_refused",
            "SELECT * FROM generate_series(1, 3)",
        ),
        Expect::Refused("table_function.generate_series.alias"),
    ));
    // FOR SHARE and FOR UPDATE OF <table>: MySQL has them (measured), MariaDB 11.4 gives a syntax
    // error for both, so it is refused rather than sent.
    let share = "SELECT id FROM users WHERE id = 4 FOR SHARE";
    let of =
        "SELECT u.id FROM users u JOIN orders o ON o.user_id = u.id WHERE o.id = 1 FOR UPDATE OF u";
    if maria {
        v.push(exp(
            c("lock_share", share),
            Expect::Refused("row_lock.share"),
        ));
        v.push(exp(c("lock_of_table", of), Expect::Refused("row_lock.of")));
    } else {
        v.push(c("lock_share", share));
        v.push(ord(c("lock_of_table", of)));
    }
    v
}

/// Known-bad controls for the `text_collation` premise. The suite above runs under `utf8mb4_bin`,
/// where text equality, ordering and min/max agree with PostgreSQL's `C` locale. Here the same
/// statements run against tables left at the server's *default* `utf8mb4` collation (MySQL
/// `utf8mb4_0900_ai_ci`, MariaDB `utf8mb4_uca1400_ai_ci`): case- and accent-insensitive, so every
/// one of them must **diverge** from PostgreSQL. A control that agrees fails the suite, because
/// then the passing runs above prove nothing about the collation.
fn known_bad_controls(maria: bool, dir: Dir) -> Vec<KnownBad> {
    const PG_PROBE: &str = "CREATE TABLE probe (id integer PRIMARY KEY, s text NOT NULL);
        INSERT INTO probe VALUES (1,'a'),(2,'B'),(3,'b'),(4,'Z');";
    let mut e = Engines::open_mysql(PG_PROBE, "CREATE TABLE _c (x integer);", maria)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    let coll = e.mysql_default_collation();
    assert!(
        !coll.is_empty(),
        "could not read the default utf8mb4 collation"
    );
    assert_ne!(
        coll, "utf8mb4_bin",
        "the control must not run under the fixture collation"
    );
    let my = format!(
        "CREATE TABLE probe (id integer PRIMARY KEY, s varchar(16) NOT NULL)          DEFAULT CHARSET=utf8mb4 COLLATE={coll};
         INSERT INTO probe VALUES (1,'a'),(2,'B'),(3,'b'),(4,'Z');"
    );
    let mut e = Engines::open_mysql(PG_PROBE, &my, maria)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    // (leaf, case name, statement, result order matters)
    let probes: [(&str, &'static str, &'static str, bool); 5] = [
        (
            "text_comparison",
            "known_bad_text_equality",
            "SELECT id FROM probe WHERE s = 'b' ORDER BY id",
            true,
        ),
        (
            "like",
            "known_bad_like_text",
            "SELECT id FROM probe WHERE s LIKE 'b' ORDER BY id",
            true,
        ),
        (
            "order_by",
            "known_bad_order_by_text",
            "SELECT s FROM probe ORDER BY s",
            true,
        ),
        (
            "fn:max",
            "known_bad_max_text",
            "SELECT max(s) FROM probe",
            false,
        ),
        (
            "fn:min",
            "known_bad_min_text",
            "SELECT min(s) FROM probe",
            false,
        ),
    ];
    let mut out = Vec::new();
    for (leaf, name, sql, ordered) in probes {
        let needs = unisql::analyse(sql, "postgres").unwrap().constructs;
        assert!(
            needs.contains(leaf),
            "control `{name}` does not use leaf `{leaf}`: {needs:?}"
        );
        let case = Case {
            name,
            sql,
            params: vec![],
            allow: ALL.to_vec(),
            ordered,
            after: None,
            hold: None,
            schema: None,
            expect: Expect::Diverges,
        };
        let o = run_case_dir(&mut e, &case, dir);
        assert!(
            o.passed,
            "known-bad control `{name}` did NOT diverge under {coll}: {}",
            o.detail
        );
        out.push(KnownBad {
            leaf,
            config: format!("collation={coll};schema=none"),
            case: name,
        });
    }
    out
}

fn run(maria: bool) {
    let mut e = Engines::open_mysql(PG, MYSQL, maria)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    let (pgv, myv) = (e.pg_version(), e.mysql_version());
    let dir = if maria {
        Dir::PgToMariadb
    } else {
        Dir::PgToMysql
    };
    let cs = cases(maria);
    let outcomes: Vec<_> = cs.iter().map(|c| run_case_dir(&mut e, c, dir)).collect();

    let mut log = String::new();
    writeln!(
        log,
        "postgres {pgv} -> {} {myv}; cases={}",
        dir.target(),
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
    let known_bad = known_bad_controls(maria, dir);
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
    let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let _ = std::fs::write(
        docs.join(format!("DIFFERENTIAL-postgres-{}.txt", dir.target())),
        &log,
    );
    let v = myv
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('-')
        .next()
        .unwrap_or("")
        .to_string();
    let _ = std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../unisql-dialect-mysql/matrix-{}-from-postgres.tsv",
            dir.target()
        )),
        unisql_differential::matrix_tsv(&outcomes, dir.target(), &v, "postgres", &known_bad),
    );
    println!("{log}");
    let failed: Vec<_> = outcomes.iter().filter(|o| !o.passed).collect();
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

#[test]
fn postgres_to_mysql_differential() {
    run(false);
}

#[test]
fn postgres_to_mariadb_differential() {
    run(true);
}
