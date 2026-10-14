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
CREATE TABLE events (id integer PRIMARY KEY, at timestamptz NOT NULL);
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
CREATE TABLE events (id integer PRIMARY KEY, at datetime NOT NULL);
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
    A::TimestampArithmeticAsText,
    A::IntegerDivisionAssumed,
    A::UpsertAnyUniqueKey,
    A::AggregateStringTruncation,
    A::ArrayNullSemantics,
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
                "refuse_upsert_where",
                "INSERT INTO kv (key, value, n) VALUES ('a','w',5) ON CONFLICT (key) DO UPDATE SET n = EXCLUDED.n WHERE kv.n < EXCLUDED.n",
            ),
            Expect::Refused("upsert_where"),
        ),
        exp(
            c(
                "refuse_filter",
                "SELECT count(*) FILTER (WHERE status = 'paid') FROM orders",
            ),
            Expect::Refused("filter_clause"),
        ),
        exp(
            c("refuse_json", "SELECT id, data->>'a' FROM users"),
            Expect::Refused("json_get_text"),
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
        exp(
            c(
                "refuse_update_from",
                "UPDATE orders SET status = 'x' FROM users WHERE users.id = orders.user_id",
            ),
            Expect::Refused("update_from"),
        ),
        exp(
            c("refuse_cast_uuid", "SELECT CAST(name AS UUID) FROM users"),
            Expect::Refused("cast.to_uuid"),
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
            config: format!("collation={coll}"),
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
