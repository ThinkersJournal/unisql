//! SQLite -> PostgreSQL differential suite. The statements are written in SQLite and run on a real
//! SQLite at its **default** settings (case-insensitive LIKE); the translation runs on a real
//! PostgreSQL over an equivalent schema (integer 0/1 flags, as SQLite stores them).
//!
//! Needs the same PostgreSQL as the other suite; fails loudly if it is unreachable.

use std::fmt::Write as _;

use unisql::Allowance as A;
use unisql_differential::{run_case_dir, Case, Dir, Engines, Expect, Param};

const PG: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active integer NOT NULL, score double precision, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (key text PRIMARY KEY, value text, n integer);
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

const LITE: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active integer NOT NULL, score real, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (key text PRIMARY KEY, value text, n integer);
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

fn c(name: &'static str, sql: &'static str, allow: &[A]) -> Case {
    Case { name, sql, params: vec![], allow: allow.to_vec(), ordered: false, after: None, expect: Expect::Same }
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
    let kvs = "SELECT key, value, n FROM kv ORDER BY key";
    vec![
        // ---- placeholders in every SQLite form
        p(c("ph_question", "SELECT id, name FROM users WHERE id = ?", &[]), vec![Param::Int(3)]),
        p(c("ph_numbered_reorder", "SELECT id FROM users WHERE id = ?2 OR id = ?1 OR id = ?2", &[]), vec![Param::Int(1), Param::Int(3)]),
        p(c("ph_named", "SELECT id FROM users WHERE id = :a OR id = :b OR id = :a", &[]), vec![Param::Int(2), Param::Int(4)]),
        p(c("ph_mixed_anon", "SELECT id FROM users WHERE age > ? AND city = ?", COLL), vec![Param::Int(26), Param::Text("Oslo")]),
        // (`age > ?` has an untyped operand, so it carries the collation allowance)
        // ---- plain exact
        c("select_where", "SELECT id FROM users WHERE city = 'Oslo' AND active = 1", &[]),
        c("is_null", "SELECT id FROM users WHERE age IS NULL OR city IS NULL", &[]),
        c("in_between", "SELECT id FROM users WHERE id IN (1,3,5) AND age BETWEEN 25 AND 41", COLL),
        c("arith_case", "SELECT id, age * 2 + 1, CASE WHEN age >= 40 THEN 'senior' ELSE 'adult' END FROM users WHERE age IS NOT NULL", COLL),
        c("aggregates", "SELECT count(*), sum(amount), min(amount), max(amount) FROM orders", COLL),
        c("group_having", "SELECT user_id, count(*), sum(amount) FROM orders GROUP BY user_id HAVING sum(amount) > 100", COLL),
        c("join", "SELECT u.name, o.amount FROM users u JOIN orders o ON o.user_id = u.id WHERE o.status = 'paid'", &[]),
        c("left_join", "SELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id", &[]),
        c("subquery_in", "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE amount > 100)", COLL),
        c("cte", "WITH big AS (SELECT user_id, sum(amount) AS t FROM orders GROUP BY user_id) SELECT u.name, big.t FROM users u JOIN big ON big.user_id = u.id", &[]),
        ord(c("with_recursive", "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 5) SELECT n FROM t ORDER BY n", COLL)),
        ord(c("window_rank", "SELECT id, rank() OVER (PARTITION BY city ORDER BY age) FROM users ORDER BY id", COLL)),
        c("agg_filter", "SELECT count(*) FILTER (WHERE status = 'paid'), count(*) FROM orders", &[]),
        c("set_ops", "SELECT id FROM users EXCEPT SELECT user_id FROM orders", &[]),
        // ---- NULL ordering: SQLite NULLs are SMALLEST, PostgreSQL's are LARGEST
        ord(c("nulls_asc_default", "SELECT id, age FROM users ORDER BY age, id", COLL)),
        ord(c("nulls_desc_default", "SELECT id, age FROM users ORDER BY age DESC, id", COLL)),
        ord(c("nulls_explicit_last", "SELECT id, age FROM users ORDER BY age NULLS LAST, id", COLL)),
        // ---- LIMIT -1 means unlimited on SQLite
        ord(c("limit_minus_one_offset", "SELECT id FROM users ORDER BY id LIMIT -1 OFFSET 4", COLL)),
        ord(c("limit_offset", "SELECT id FROM users ORDER BY id LIMIT 2 OFFSET 1", COLL)),
        // ---- LIKE: case-insensitive by default on SQLite; no default escape character
        c("like_ci_default", "SELECT name FROM users WHERE name LIKE 'BOB'", &like),
        c("like_prefix_ci", "SELECT name FROM users WHERE name LIKE 'A%'", &like),
        c("like_no_escape_backslash", "SELECT name FROM users WHERE name LIKE 'frank\\%'", &like),
        c("like_custom_escape", "SELECT name FROM users WHERE name LIKE 'Eve!_X' ESCAPE '!'", &like),
        c("not_like", "SELECT id FROM users WHERE name NOT LIKE '%A%'", &like),
        // ---- function renames and mappings
        c("ifnull_coalesce", "SELECT id, ifnull(city, 'none') FROM users", &[]),
        c("group_concat_sep", "SELECT group_concat(name, ',') FROM users WHERE id = 1", &[]),
        c("group_concat_default", "SELECT group_concat(name) FROM users WHERE id = 1", &[]),
        c("string_funcs", "SELECT length(name), replace(name, 'a', 'A'), trim('  x '), ltrim('  x'), lower(name), upper(name) FROM users", &[A::AsciiOnlyCaseFolding]),
        c("current_timestamp", "SELECT id FROM users WHERE CURRENT_TIMESTAMP IS NOT NULL AND id = 1", &[A::NowAsText]),
        c("int_division", "SELECT 7 / 2, -7 / 2, 7 % 3, -7 % 3 FROM users WHERE id = 1", &[A::DivisionByZeroIsNull]),
        c("cast_ok", "SELECT CAST(id AS TEXT), CAST('12' AS INTEGER) FROM users WHERE id = 1", &[A::LenientCast]),
        // ---- DML
        after(p(c("insert_values", "INSERT INTO users (id, name, active) VALUES (?, ?, 1)", &[]), vec![Param::Int(7), Param::Text("gus")]),
            "SELECT id, name, active FROM users WHERE id = 7"),
        c("insert_returning", "INSERT INTO kv (key, value, n) VALUES ('d', 'w', 9) RETURNING key, n", &[]),
        after(p(c("upsert", "INSERT INTO kv (key, value, n) VALUES (?, ?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value, n = excluded.n", &[]),
            vec![Param::Text("a"), Param::Text("new"), Param::Int(11)]), kvs),
        after(c("upsert_do_nothing", "INSERT INTO kv (key, value, n) VALUES ('a', 'x', 0) ON CONFLICT (key) DO NOTHING", &[]), kvs),
        c("update_returning", "UPDATE users SET age = age + 1 WHERE city = 'Oslo' RETURNING id, age", &[]),
        after(c("delete_where", "DELETE FROM orders WHERE amount < 100", &[]), "SELECT id FROM orders ORDER BY id"),
        // ---- the allowance gate, source side
        exp(c("gate_like_source", "SELECT name FROM users WHERE name LIKE 'a%'", &[]), Expect::NeedsAllowance(A::LikeCaseSensitivityIsConnectionSetting)),
        exp(c("gate_current_timestamp", "SELECT CURRENT_TIMESTAMP", &[]), Expect::NeedsAllowance(A::NowAsText)),
        exp(c("gate_division", "SELECT 1 / 2", &[]), Expect::NeedsAllowance(A::DivisionByZeroIsNull)),
        // ---- known divergences: the allowance is needed because the engines disagree
        exp(c("diverge_division_by_zero", "SELECT 1 / 0", &[A::DivisionByZeroIsNull]), Expect::Diverges),
        exp(c("diverge_bad_cast", "SELECT CAST('abc' AS INTEGER)", &[A::LenientCast]), Expect::Diverges),
        // ---- refusals: SQLite-only functions have no verified PostgreSQL mapping
        exp(c("refuse_random", "SELECT random()", &[]), Expect::Refused("fn:random")),
        exp(c("refuse_typeof", "SELECT typeof(id) FROM users", &[]), Expect::Refused("fn:typeof")),
        exp(c("refuse_strftime", "SELECT strftime('%Y', 'now')", &[]), Expect::Refused("fn:strftime")),
        exp(c("refuse_instr", "SELECT instr(name, 'a') FROM users", &[]), Expect::Refused("fn:instr")),
        // ---- not in the IR
        exp(c("ir_insert_or_replace", "INSERT OR REPLACE INTO kv (key, value) VALUES ('a', 'b')", &[]), Expect::NotInIr("insert_option")),
        exp(c("ir_glob", "SELECT name FROM users WHERE name GLOB 'a*'", &[]), Expect::NotInIr("binary_operator")),
    ]
}

#[test]
fn sqlite_to_postgres_differential() {
    let mut e = Engines::open_with(PG, LITE, false)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    let (pgv, litev) = (e.pg_version(), e.sqlite_version());
    let cs = cases();
    let outcomes: Vec<_> = cs.iter().map(|c| run_case_dir(&mut e, c, Dir::SqliteToPg)).collect();

    let mut log = String::new();
    writeln!(log, "sqlite {litev} -> postgres {pgv}; cases={}", outcomes.len()).unwrap();
    for o in &outcomes {
        writeln!(log, "{} {:<28} {}", if o.passed { "PASS" } else { "FAIL" }, o.name, o.detail.lines().next().unwrap_or("")).unwrap();
        if let Some(t) = &o.translated {
            writeln!(log, "       -> {t}").unwrap();
        }
    }
    let matrix = unisql_differential::matrix_rows(&outcomes);
    writeln!(
        log,
        "\nMATRIX (construct, tier) from {} passing translated cases:",
        outcomes.iter().filter(|o| o.passed && o.translated.is_some()).count()
    )
    .unwrap();
    for (k, t) in &matrix {
        writeln!(log, "  {k:<28} {t}").unwrap();
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let _ = std::fs::write(dir.join("DIFFERENTIAL-sqlite-postgres.txt"), &log);
    println!("{log}");
    let failed: Vec<_> = outcomes.iter().filter(|o| !o.passed).collect();
    if !failed.is_empty() {
        for o in &failed {
            println!("---- FAILED {}\n{}", o.name, o.detail);
        }
        panic!("{} of {} differential cases failed", failed.len(), outcomes.len());
    }
}
