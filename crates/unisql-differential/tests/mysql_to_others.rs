// SPDX-License-Identifier: MIT OR Apache-2.0
//! MySQL / MariaDB -> PostgreSQL and -> SQLite differential suites (the MySQL reader).
//!
//! Statements are written in MySQL and run on a real MySQL 8.4 / MariaDB 11.4 started with
//! `utf8mb4_bin` (the premise of `text_collation`); the translation runs on a real PostgreSQL (`C`
//! locale) or SQLite over an equivalent schema (integer 0/1 flags, as MySQL stores TINYINT(1)).
//! Fails loudly if an engine is unreachable.

use std::fmt::Write as _;

use unisql::Allowance as A;
use unisql_differential::{Case, Dir, Engines, Expect, Param, run_case_dir};

const PG: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active integer NOT NULL, score double precision, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (k text PRIMARY KEY, v text, n integer);
CREATE TABLE events (id integer PRIMARY KEY, at timestamptz NOT NULL);
INSERT INTO events VALUES (1, now() - interval '10 seconds'),(2, now() - interval '120 seconds'),(3, now() - interval '3600 seconds');
INSERT INTO users VALUES
 (1,'alice','alice@example.com',30,1,9.5,'Oslo'),
 (2,'Bob','bob@example.com',25,0,7.25,'Bergen'),
 (3,'carol',NULL,41,1,NULL,'Oslo'),
 (4,'dave','Dave@Example.com',NULL,1,3.0,NULL),
 (5,'Eve_X','eve@example.com',25,0,8.0,'Bergen'),
 (6,'frank%','f@x.org',60,1,1.5,'Trondheim'),
 (7,'h' || chr(233) || 'llo','h@x.org',33,1,2.0,'Oslo');
INSERT INTO orders VALUES (1,1,100,'paid'),(2,1,250,'paid'),(3,2,75,'open'),(4,3,0,'void'),(5,3,500,'paid'),(6,5,30,'open');
INSERT INTO kv VALUES ('a','1',1),('b',NULL,2),('c','x',NULL),('Cap','z',3);
"#;

const LITE: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name text NOT NULL, email text, age integer,
                    active integer NOT NULL, score real, city text);
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL REFERENCES users(id),
                     amount integer NOT NULL, status text NOT NULL);
CREATE TABLE kv (k text PRIMARY KEY, v text, n integer);
CREATE TABLE events (id integer PRIMARY KEY, at text NOT NULL);
INSERT INTO events VALUES (1, datetime('now','-10 seconds')),(2, datetime('now','-120 seconds')),(3, datetime('now','-3600 seconds'));
INSERT INTO users VALUES
 (1,'alice','alice@example.com',30,1,9.5,'Oslo'),
 (2,'Bob','bob@example.com',25,0,7.25,'Bergen'),
 (3,'carol',NULL,41,1,NULL,'Oslo'),
 (4,'dave','Dave@Example.com',NULL,1,3.0,NULL),
 (5,'Eve_X','eve@example.com',25,0,8.0,'Bergen'),
 (6,'frank%','f@x.org',60,1,1.5,'Trondheim'),
 (7,'h' || char(233) || 'llo','h@x.org',33,1,2.0,'Oslo');
INSERT INTO orders VALUES (1,1,100,'paid'),(2,1,250,'paid'),(3,2,75,'open'),(4,3,0,'void'),(5,3,500,'paid'),(6,5,30,'open');
INSERT INTO kv VALUES ('a','1',1),('b',NULL,2),('c','x',NULL),('Cap','z',3);
"#;

const MYSQL: &str = r#"
CREATE TABLE users (id integer PRIMARY KEY, name varchar(64) NOT NULL, email varchar(64), age integer,
                    active tinyint(1) NOT NULL, score double, city varchar(64));
CREATE TABLE orders (id integer PRIMARY KEY, user_id integer NOT NULL, amount integer NOT NULL, status varchar(16) NOT NULL,
                     FOREIGN KEY (user_id) REFERENCES users(id));
CREATE TABLE kv (k varchar(64) PRIMARY KEY, v varchar(64), n integer);
CREATE TABLE events (id integer PRIMARY KEY, at datetime NOT NULL);
INSERT INTO events VALUES (1, now() - interval 10 second),(2, now() - interval 120 second),(3, now() - interval 3600 second);
INSERT INTO users VALUES
 (1,'alice','alice@example.com',30,1,9.5,'Oslo'),
 (2,'Bob','bob@example.com',25,0,7.25,'Bergen'),
 (3,'carol',NULL,41,1,NULL,'Oslo'),
 (4,'dave','Dave@Example.com',NULL,1,3.0,NULL),
 (5,'Eve_X','eve@example.com',25,0,8.0,'Bergen'),
 (6,'frank%','f@x.org',60,1,1.5,'Trondheim'),
 (7,concat('h', convert(0xC3A9 using utf8mb4), 'llo'),'h@x.org',33,1,2.0,'Oslo');
INSERT INTO orders VALUES (1,1,100,'paid'),(2,1,250,'paid'),(3,2,75,'open'),(4,3,0,'void'),(5,3,500,'paid'),(6,5,30,'open');
INSERT INTO kv VALUES ('a','1',1),('b',NULL,2),('c','x',NULL),('Cap','z',3);
"#;

/// Everything a translation from MySQL may legitimately need.
const ALL: &[A] = &[
    A::TextCollation,
    A::LikeCaseSensitivityIsConnectionSetting,
    A::AsciiOnlyCaseFolding,
    A::LenientCast,
    A::DivisionByZeroIsNull,
    A::DecimalDivisionAsFloat,
    A::IntegerDivisionAssumed,
    A::AggregateStringTruncation,
    A::BooleanAsInteger,
    A::NowAsText,
    A::DateAsText,
    A::TimestampAsText,
    A::TimestampMillisecondResolution,
    A::ReplaceIsUpsert,
    A::LenientCast,
    A::TimestampArithmeticAsText,
    A::PipesAsLogicalOr,
    A::IgnoreCoversOnlyUniqueConflicts,
    A::SubstringStartAssumedPositive,
    A::RegexpFlavor,
];

/// `kv` as the fixtures define it: columns k, v, n; one unique key (k).
fn kv_schema() -> unisql::Schema {
    let mut s = unisql::Schema::new()
        .with_column("kv", "k", unisql::Column::text())
        .with_column("kv", "v", unisql::Column::text())
        .with_column("kv", "n", unisql::Column::numeric())
        .with_unique_key("kv", &["k"]);
    s.mark_columns_complete(vec!["kv".to_string()]);
    s
}

fn c_only(name: &'static str, sql: &'static str, allow: &[A]) -> Case {
    Case {
        allow: allow.to_vec(),
        ..c(name, sql)
    }
}

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
fn only(mut x: Case, allow: &[A]) -> Case {
    x.allow = allow.to_vec();
    x
}

fn cases(dir: Dir) -> Vec<Case> {
    let mut v = vec![
        // ---- placeholders (positional ?), backtick identifiers
        p(
            c("ph_question", "SELECT id, name FROM users WHERE id = ?"),
            vec![Param::Int(3)],
        ),
        p(
            c("ph_two", "SELECT id FROM users WHERE age > ? AND city = ?"),
            vec![Param::Int(26), Param::Text("Oslo")],
        ),
        c(
            "backticks",
            "SELECT `id`, `name` FROM `users` WHERE `age` BETWEEN 25 AND 41",
        ),
        // ---- exact core
        c(
            "select_where",
            "SELECT id FROM users WHERE city = 'Oslo' AND active = 1",
        ),
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
            "in_list",
            "SELECT id FROM users WHERE id IN (1, 3, 5) AND id NOT IN (5)",
        ),
        c(
            "aggregates",
            "SELECT count(*), count(status), sum(amount), min(amount), max(amount) FROM orders",
        ),
        c(
            "group_having",
            "SELECT user_id, count(*), sum(amount) FROM orders GROUP BY user_id HAVING sum(amount) > 100",
        ),
        c(
            "join",
            "SELECT u.name, o.amount FROM users u JOIN orders o ON o.user_id = u.id WHERE o.status = 'paid'",
        ),
        c(
            "left_join",
            "SELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id",
        ),
        c(
            "subquery_in",
            "SELECT name FROM users WHERE id IN (SELECT user_id FROM orders WHERE amount > 100)",
        ),
        c(
            "exists",
            "SELECT name FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND o.status = 'open')",
        ),
        c(
            "cte",
            "WITH big AS (SELECT user_id, sum(amount) AS t FROM orders GROUP BY user_id) SELECT u.name, big.t FROM users u JOIN big ON big.user_id = u.id",
        ),
        ord(c(
            "with_recursive",
            "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 5) SELECT n FROM t ORDER BY n",
        )),
        ord(c(
            "window_rank",
            "SELECT id, rank() OVER (PARTITION BY city ORDER BY age) FROM users ORDER BY id",
        )),
        c(
            "set_ops",
            "SELECT id FROM users EXCEPT SELECT user_id FROM orders",
        ),
        // ---- NULL ordering: MySQL NULLs are the SMALLEST value
        ord(c("nulls_asc", "SELECT id, age FROM users ORDER BY age, id")),
        ord(c(
            "nulls_desc",
            "SELECT id, age FROM users ORDER BY age DESC, id",
        )),
        // ---- LIMIT forms
        ord(c(
            "limit_comma",
            "SELECT id FROM users ORDER BY id LIMIT 2, 3",
        )),
        ord(c(
            "limit_offset",
            "SELECT id FROM users ORDER BY id LIMIT 3 OFFSET 2",
        )),
        // ---- MySQL operator and function semantics
        c("or_keyword", "SELECT id FROM users WHERE id = 1 OR id = 2"),
        c("div_decimal", "SELECT 7 / 2, 1 / 4 FROM users WHERE id = 1"),
        c(
            "div_integer",
            "SELECT 7 DIV 2, -7 DIV 2, 7 % 3, -7 % 3 FROM users WHERE id = 1",
        ),
        c(
            "ifnull_concat_if",
            "SELECT concat(name, '-', ifnull(city, '?')), IF(age >= 40, 'senior', 'adult') FROM users",
        ),
        c(
            "length_is_bytes",
            "SELECT length(name), char_length(name) FROM users WHERE id IN (1, 7)",
        ),
        c(
            "group_concat_sep",
            "SELECT group_concat(name ORDER BY age, id SEPARATOR '|') FROM users",
        ),
        c(
            "group_concat_default",
            "SELECT group_concat(name) FROM users WHERE id = 1",
        ),
        c(
            "cast_forms",
            "SELECT CAST(id AS SIGNED), CAST(id AS CHAR) FROM users WHERE id = 1",
        ),
        // ---- LIKE (default escape is a backslash, as in PostgreSQL)
        c("like_prefix", "SELECT name FROM users WHERE name LIKE 'a%'"),
        c(
            "like_escape",
            "SELECT name FROM users WHERE name LIKE 'frank\\%'",
        ),
        // ---- DML
        after(
            p(
                c(
                    "insert_values",
                    "INSERT INTO users (id, name, active) VALUES (?, ?, 1)",
                ),
                vec![Param::Int(9), Param::Text("gus")],
            ),
            "SELECT id, name, active FROM users WHERE id = 9",
        ),
        after(
            c("delete_where", "DELETE FROM orders WHERE amount < 100"),
            "SELECT id FROM orders ORDER BY id",
        ),
        after(
            c(
                "update_where",
                "UPDATE users SET age = age + 1 WHERE city = 'Oslo'",
            ),
            "SELECT id, age FROM users ORDER BY id",
        ),
        // ---- the gate, source side
        exp(
            only(c("gate_div_decimal", "SELECT 7 / 2"), &[A::TextCollation]),
            Expect::NeedsAllowance(A::DecimalDivisionAsFloat),
        ),
        // ---- refusals and not-in-IR
        exp(
            c(
                "refuse_odku",
                "INSERT INTO kv (k, v) VALUES ('a', 'b') ON DUPLICATE KEY UPDATE v = VALUES(v)",
            ),
            Expect::Refused("upsert.no_conflict_key"),
        ),
        // ---- the conflict key from the schema: kv has exactly one unique key, `k`
        Case {
            schema: Some(unisql::Schema::new().with_unique_key("kv", &["k"])),
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "odku_key_from_schema_duplicate",
                "INSERT INTO kv (k, v, n) VALUES ('a', 'dup', 99) ON DUPLICATE KEY UPDATE v = VALUES(v), n = n + 1",
            )
        },
        Case {
            schema: Some(unisql::Schema::new().with_unique_key("kv", &["k"])),
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "odku_key_from_schema_new",
                "INSERT INTO kv (k, v, n) VALUES ('new', 'x', 5) ON DUPLICATE KEY UPDATE v = VALUES(v)",
            )
        },
        exp(
            Case {
                schema: Some(
                    unisql::Schema::new()
                        .with_unique_key("kv", &["k"])
                        .with_unique_key("kv", &["n"]),
                ),
                ..c(
                    "refuse_odku_two_keys",
                    "INSERT INTO kv (k, v) VALUES ('a', 'b') ON DUPLICATE KEY UPDATE v = VALUES(v)",
                )
            },
            Expect::Refused("upsert.no_conflict_key"),
        ),
        // ---- MySQL-only forms the reader now accepts (each with the assumption it carries)
        Case {
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "insert_ignore_duplicate",
                "INSERT IGNORE INTO kv (k, v, n) VALUES ('a', 'dup', 99)",
            )
        },
        Case {
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "insert_ignore_new",
                "INSERT IGNORE INTO kv (k, v, n) VALUES ('new', 'x', 5)",
            )
        },
        c("pipes_as_or", "SELECT id FROM users WHERE id = 1 || id = 2"),
        c(
            "pipes_inside_string_untouched",
            "SELECT id FROM users WHERE name = 'a||b' || id = 3",
        ),
        c(
            "null_safe_eq_null",
            "SELECT id FROM users WHERE age <=> NULL",
        ),
        c(
            "null_safe_eq_value",
            "SELECT id FROM users WHERE age <=> 25",
        ),
        // A start that is an expression: the engines agree here because every id is >= 1; that this
        // is so is the caller's stated assumption (the allowance).
        c(
            "substring_expression_start",
            "SELECT substring(name, id, 2) FROM users",
        ),
        c(
            "substring_positive_start",
            "SELECT substring(name, 2, 3) FROM users",
        ),
        exp(
            c(
                "substring_zero_start_diverges",
                "SELECT substring(name, 0, 3) FROM users",
            ),
            Expect::Diverges,
        ),
        exp(
            c_only(
                "gate_substring_zero_start",
                "SELECT substring(name, 0, 3) FROM users",
                &[],
            ),
            Expect::NeedsAllowance(A::SubstringStartAssumedPositive),
        ),
        exp(
            c_only(
                "gate_pipes",
                "SELECT id FROM users WHERE id = 1 || id = 2",
                &[],
            ),
            Expect::NeedsAllowance(A::PipesAsLogicalOr),
        ),
        exp(
            c_only(
                "gate_insert_ignore",
                "INSERT IGNORE INTO kv (k, v) VALUES ('a', 'b')",
                &[],
            ),
            Expect::NeedsAllowance(A::IgnoreCoversOnlyUniqueConflicts),
        ),
        c(
            "current_date_bare",
            "SELECT CAST(CURRENT_DATE AS CHAR) FROM users WHERE id = 1",
        ),
        c(
            "current_date_call",
            "SELECT CAST(CURRENT_DATE() AS CHAR) FROM users WHERE id = 1",
        ),
        // INTERVAL n UNIT is the make_interval form on every target.
        c(
            "interval_minutes",
            "SELECT id FROM events WHERE at > NOW() - INTERVAL 5 MINUTE",
        ),
        c(
            "interval_hours_plus",
            "SELECT id FROM events WHERE at < NOW() + INTERVAL 2 HOUR",
        ),
        exp(
            c("refuse_interval_month", "SELECT NOW() - INTERVAL 1 MONTH"),
            Expect::Refused("fn:make_interval.month_year"),
        ),
        c(
            "join_using",
            "SELECT id, amount FROM users JOIN orders USING (id) ORDER BY id",
        ),
        c(
            "left_join_using",
            "SELECT id, amount FROM users LEFT JOIN orders USING (id) ORDER BY id",
        ),
        c(
            "natural_join",
            "SELECT id, amount FROM users NATURAL JOIN orders ORDER BY id",
        ),
        c(
            "natural_left_join",
            "SELECT id, amount FROM users NATURAL LEFT JOIN orders ORDER BY id",
        ),
        Case {
            schema: Some(kv_schema()),
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "replace_into_existing",
                "REPLACE INTO kv (k, v) VALUES ('a', 'rep')",
            )
        },
        Case {
            schema: Some(kv_schema()),
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "replace_into_new",
                "REPLACE INTO kv (k, v, n) VALUES ('fresh', 'x', 9)",
            )
        },
        Case {
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c("insert_set", "INSERT INTO kv SET k = 'sx', v = 'y', n = 7")
        },
        Case {
            after: Some("SELECT * FROM kv ORDER BY 1"),
            ..c(
                "insert_set_partial_columns",
                "INSERT INTO kv SET k = 'sy', n = 1 + 1",
            )
        },
        c(
            "curdate_is_current_date",
            "SELECT CAST(CURDATE() AS CHAR) FROM users WHERE id = 1",
        ),
        c(
            "utc_timestamp_window",
            "SELECT id FROM events WHERE at > UTC_TIMESTAMP() - INTERVAL 5 MINUTE ORDER BY id",
        ),
        c(
            "utc_timestamp6_window",
            "SELECT id FROM events WHERE at > UTC_TIMESTAMP(6) - INTERVAL 5 MINUTE ORDER BY id",
        ),
        c(
            "utc_timestamp_future",
            "SELECT COUNT(*) FROM events WHERE at < UTC_TIMESTAMP()",
        ),
        c(
            "utc_date_text",
            "SELECT CAST(UTC_DATE() AS CHAR) FROM users WHERE id = 1",
        ),
        c(
            "datediff_literals",
            "SELECT DATEDIFF('2024-03-01', '2024-02-01'), DATEDIFF('2024-01-01 23:59:59', '2024-01-02 00:00:01')",
        ),
        c(
            "date_part",
            "SELECT CAST(DATE('2024-05-06 07:08:09') AS CHAR)",
        ),
        c(
            "concat_ws_skips_null",
            "SELECT id, CONCAT_WS('-', name, email, city) FROM users ORDER BY id",
        ),
        c(
            "left_right_literal",
            "SELECT id, LEFT(name, 2), RIGHT(name, 3), LEFT(name, 0), RIGHT(name, 0), LEFT(name, 99), RIGHT(name, 99) FROM users ORDER BY id",
        ),
        c(
            "repeat_literal",
            "SELECT id, REPEAT(name, 2), REPEAT(name, 0) FROM users ORDER BY id",
        ),
        exp(
            c("refuse_left_negative", "SELECT LEFT(name, -1) FROM users"),
            Expect::Refused("fn:left.length"),
        ),
        exp(
            c("refuse_left_column", "SELECT LEFT(name, age) FROM users"),
            Expect::Refused("fn:left.length"),
        ),
        exp(
            c("refuse_utc_timestamp_precision", "SELECT UTC_TIMESTAMP(3)"),
            Expect::Refused("fn:utc_timestamp.precision"),
        ),
        exp(
            c("refuse_date_format", "SELECT date_format(now(), '%Y')"),
            Expect::Refused("fn:date_format"),
        ),
    ];
    if matches!(dir, Dir::MysqlToSqlite) {
        v.push(exp(
            c(
                "refuse_regexp",
                "SELECT id FROM users WHERE city REGEXP '^O'",
            ),
            Expect::Refused("regexp"),
        ));
    }
    if matches!(dir, Dir::MysqlToPg | Dir::MariadbToPg) {
        // MySQL `1/0` is NULL; PostgreSQL raises an error. (SQLite agrees with MySQL here.)
        v.push(exp(c("diverge_div_zero", "SELECT 1 / 0"), Expect::Diverges));
        v.push(c(
            "lpad_rpad_literal",
            "SELECT id, LPAD(name, 8, '*'), LPAD(name, 3, 'ab'), RPAD(name, 8, 'xy'), RPAD(name, 2, '.') FROM users ORDER BY id",
        ));
        v.push(c(
            "reverse_text",
            "SELECT id, REVERSE(name) FROM users ORDER BY id",
        ));
        v.push(c(
            "regexp",
            "SELECT id FROM users WHERE city REGEXP '^O.l' OR name RLIKE 'a$'",
        ));
        v.push(c(
            "not_regexp",
            "SELECT id FROM users WHERE city NOT REGEXP '^O.l'",
        ));
        v.push(exp(
            c(
                "refuse_lpad_empty_pad",
                "SELECT LPAD(name, 8, '') FROM users",
            ),
            Expect::Refused("fn:lpad.args"),
        ));
        // No schema: PostgreSQL has no REPLACE and the columns and key are unknown.
        v.push(exp(
            c(
                "refuse_replace_no_schema",
                "REPLACE INTO kv (k, v) VALUES ('a', 'rep')",
            ),
            Expect::Refused("replace_into"),
        ));
    }
    v
}

fn run(dir: Dir, maria: bool) {
    let lite_case_sensitive = true;
    let mut e = Engines::open_mysql_with(PG, LITE, MYSQL, maria, lite_case_sensitive)
        .unwrap_or_else(|m| panic!("ENGINE UNAVAILABLE (test must not be skipped): {m}"));
    let (pgv, litev, myv) = (e.pg_version(), e.sqlite_version(), e.mysql_version());
    let cs = cases(dir);
    let outcomes: Vec<_> = cs.iter().map(|c| run_case_dir(&mut e, c, dir)).collect();

    let mut log = String::new();
    writeln!(
        log,
        "{} {myv} -> {} (pg {pgv}, sqlite {litev}); cases={}",
        dir.source(),
        dir.target(),
        outcomes.len()
    )
    .unwrap();
    for o in &outcomes {
        writeln!(
            log,
            "{} {:<26} {}",
            if o.passed { "PASS" } else { "FAIL" },
            o.name,
            o.detail.lines().next().unwrap_or("")
        )
        .unwrap();
        if let Some(t) = &o.translated {
            writeln!(log, "       -> {t}").unwrap();
        }
    }
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
        docs.join(format!(
            "DIFFERENTIAL-{}-{}.txt",
            dir.source(),
            dir.target()
        )),
        &log,
    );
    let backend_module = if dir.target() == "sqlite" {
        "sqlite"
    } else {
        "postgres"
    };
    let backend_version = if dir.target() == "sqlite" {
        litev.clone()
    } else {
        pgv.split_whitespace().next().unwrap_or("").to_string()
    };
    let _ = std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../unisql-dialect-{backend_module}/matrix-{}-from-{}.tsv",
            dir.target(),
            dir.source()
        )),
        unisql_differential::matrix_tsv(
            &outcomes,
            dir.target(),
            &backend_version,
            dir.source(),
            &[],
        ),
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
fn mysql_to_postgres_differential() {
    run(Dir::MysqlToPg, false);
}

#[test]
fn mysql_to_sqlite_differential() {
    run(Dir::MysqlToSqlite, false);
}

#[test]
fn mariadb_to_postgres_differential() {
    run(Dir::MariadbToPg, true);
}
