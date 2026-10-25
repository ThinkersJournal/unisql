// SPDX-License-Identifier: MIT OR Apache-2.0
//! A placeholder written into the translated text must be accounted for in `param_order`.
//!
//! A target with positional `?` (MySQL, MariaDB) binds values in exactly the order `param_order`
//! lists, so the number of `?` in the text must equal its length; a mapping that writes an
//! argument twice must either list the parameter twice or refuse. Targets with numbered
//! placeholders (`$n`, `?n`) may reuse a number, but every number written must be listed and
//! every number listed must be written. The statements below put a parameter in each argument
//! position of the mappings that exist, so a mapping that repeats an argument cannot hide.

use std::collections::BTreeSet;

use unisql::{Allowance, Options, translate};

/// Placeholders in `sql` outside single-quoted strings and double-quoted or backticked names:
/// the number for `$n` / `?n`, `None` for a bare `?`.
fn placeholders(sql: &str) -> Vec<Option<u32>> {
    let b: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            q @ ('\'' | '"' | '`') => {
                i += 1;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
            }
            '?' | '$' => {
                let mut j = i + 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                let digits: String = b[i + 1..j].iter().collect();
                if b[i] == '?' || !digits.is_empty() {
                    out.push(digits.parse().ok());
                }
                i = j - 1;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

#[test]
fn the_placeholder_scanner_counts_what_it_should() {
    // control for the scanner: two bare `?`, one inside a string ignored, numbers read
    assert_eq!(placeholders("SELECT ?, '?', ?"), vec![None, None]);
    assert_eq!(
        placeholders("SELECT ?1, \"?\", ?1, ?2"),
        vec![Some(1), Some(1), Some(2)]
    );
    assert_eq!(
        placeholders("SELECT $1 || $2 || '$3'"),
        vec![Some(1), Some(2)]
    );
}

/// Statements in the PostgreSQL dialect; `$1`, `$2`, `$3` are the parameters.
const STATEMENTS: &[&str] = &[
    "SELECT date_trunc('year', $1::timestamp)",
    "SELECT date_trunc('quarter', $1::timestamp)",
    "SELECT date_trunc('month', $1::timestamp)",
    "SELECT date_trunc('week', $1::timestamp)",
    "SELECT date_trunc('day', $1::timestamp)",
    "SELECT date_trunc('hour', $1::timestamp)",
    "SELECT date_trunc('minute', $1::timestamp)",
    "SELECT date_trunc('week', $1::timestamp), $2",
    "SELECT $1, date_trunc('quarter', $2::timestamp)",
    "SELECT to_char($1::timestamp, 'YYYY-MM-DD')",
    "SELECT date_part('year', $1::timestamp)",
    "SELECT extract(year FROM $1::timestamp)",
    "SELECT extract(month FROM $1::timestamp)",
    "SELECT extract(day FROM $1::timestamp)",
    "SELECT extract(hour FROM $1::timestamp)",
    "SELECT extract(minute FROM $1::timestamp)",
    "SELECT extract(epoch FROM $1::timestamp)",
    "SELECT extract(dow FROM $1::timestamp)",
    "SELECT extract(isodow FROM $1::timestamp)",
    "SELECT extract(doy FROM $1::timestamp)",
    "SELECT extract(quarter FROM $1::timestamp)",
    "SELECT extract(week FROM $1::timestamp)",
    "SELECT extract(epoch FROM $1::timestamp - $2::timestamp)",
    "SELECT $1::timestamp + make_interval(secs => $2)",
    "SELECT to_timestamp($1)",
    "SELECT strpos($1, $2)",
    "SELECT position($1 in $2)",
    "SELECT left($1, 2), right($2, 3)",
    "SELECT lpad($1, 5, 'x'), rpad($2, 5, 'y')",
    "SELECT reverse($1), repeat($2, 2), concat_ws('-', $1, $2, $3)",
    "SELECT btrim($1), ltrim($1), rtrim($1), btrim($1, $2)",
    "SELECT replace($1, $2, $3)",
    "SELECT lower($1), upper($1), length($1), char_length($1), octet_length($1)",
    "SELECT coalesce($1, $2), nullif($1, $2)",
    "SELECT md5($1)",
    "SELECT $1 || $2",
    "SELECT $1 ILIKE $2",
    "SELECT $1 LIKE $2",
    "SELECT $1 ~ $2",
    "SELECT CAST($1 AS integer), CAST($1 AS text), CAST($1 AS boolean), CAST($1 AS bigint)",
    "SELECT $1::integer + $2::integer, $1::integer / $2::integer, $1::integer % $2::integer",
    "SELECT ceil($1::double precision), floor($1::double precision), mod($1::integer, $2::integer)",
    "SELECT greatest($1, $2), least($1, $2)",
    "SELECT abs($1::integer)",
    "SELECT CASE WHEN $1 > 0 THEN $2 ELSE $3 END",
    "SELECT $1 BETWEEN $2 AND $3",
    "SELECT $1 IN ($2, $3)",
    "SELECT $1 = ANY($2)",
    "SELECT $1 IS DISTINCT FROM $2, $1 IS NOT DISTINCT FROM $2",
    "SELECT a FROM t WHERE a = $1 ORDER BY b NULLS FIRST LIMIT $2 OFFSET $3",
    "SELECT a FROM t ORDER BY $1 NULLS LAST",
    "SELECT a FROM t ORDER BY b + $1 DESC NULLS FIRST",
    "SELECT DISTINCT ON (a) a, b FROM t WHERE b > $1 ORDER BY a, b",
    "SELECT string_agg(a, $1) FROM t",
    "SELECT string_agg(a, $1 ORDER BY b) FROM t",
    "SELECT sum(a) FILTER (WHERE b > $1) FROM t",
    "SELECT row_number() OVER (PARTITION BY a ORDER BY b), lag(a, $1) OVER (ORDER BY b) FROM t",
    "SELECT a FROM t WHERE a > $1 FOR UPDATE",
    "SELECT a FROM t WHERE EXISTS (SELECT 1 FROM u WHERE u.x = t.a AND u.y = $1)",
    "SELECT a FROM t WHERE a IN (SELECT x FROM u WHERE y = $1)",
    "WITH c AS (SELECT a FROM t WHERE b = $1) SELECT a FROM c WHERE a > $2",
    "SELECT a FROM t UNION SELECT x FROM u WHERE y = $1",
    "INSERT INTO t (a, b) VALUES ($1, $2) RETURNING a",
    "INSERT INTO t (a, b) VALUES ($1, $2), ($3, $1)",
    "INSERT INTO t (a, b) VALUES ($1, $2) ON CONFLICT (a) DO UPDATE SET b = EXCLUDED.b",
    "INSERT INTO t (a, b) VALUES ($1, $2) ON CONFLICT (a) DO UPDATE SET b = $2 WHERE t.b < $3",
    "INSERT INTO t (a, b) VALUES ($1, $2) ON CONFLICT (a) DO UPDATE SET b = EXCLUDED.b, c = EXCLUDED.c WHERE t.b < $3",
    "INSERT INTO t (a, b) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    "UPDATE t SET a = $1, b = b + $2 WHERE c = $3 RETURNING a",
    "DELETE FROM t WHERE a = $1 AND b < $2 RETURNING a",
    "SELECT gen_random_uuid(), now(), current_date",
    "SELECT $1::uuid, $2::jsonb, json_build_object('k', $1)",
    "SELECT $1 -> 'a', $1 ->> 'b'",
    "SELECT unnest($1::integer[])",
    "SELECT * FROM generate_series(1, $1)",
    "SELECT array_agg($1)",
    "SELECT regexp_replace($1, $2, $3)",
    "SELECT split_part($1, ',', 2)",
    "SELECT substring($1 FROM 2 FOR 3), substr($1, $2)",
    "SELECT trim(both ' ' FROM $1)",
    "SELECT concat($1, $2)",
];

fn all_allowances() -> Options {
    Allowance::ALL
        .iter()
        .fold(Options::default(), |o, a| o.with(*a))
}

#[test]
fn every_placeholder_in_the_text_is_accounted_for_in_param_order() {
    let opts = all_allowances();
    let mut checked = 0usize;
    let mut bad = Vec::new();
    for sql in STATEMENTS {
        for target in ["postgres", "sqlite", "mysql", "mariadb"] {
            let Ok(t) = translate(sql, "postgres", target, &opts) else {
                continue; // a refusal binds nothing wrongly
            };
            checked += 1;
            let found = placeholders(&t.sql);
            let listed: BTreeSet<u32> = t.param_order.iter().copied().collect();
            let ok = if matches!(target, "mysql" | "mariadb") {
                // positional: one value per `?`, in `param_order`
                found.len() == t.param_order.len() && found.iter().all(Option::is_none)
            } else {
                // numbered: the numbers written are exactly the parameters listed
                let written: BTreeSet<u32> = found.iter().flatten().copied().collect();
                found.iter().all(Option::is_some) && written == listed
            };
            if !ok {
                bad.push(format!(
                    "{target}: {sql}\n    -> {}\n    placeholders={} param_order={:?}",
                    t.sql,
                    found.len(),
                    t.param_order
                ));
            }
        }
    }
    // control: the sweep reached most translations, so a green result is not an empty loop
    assert!(checked > 150, "only {checked} translations were checked");
    assert!(
        bad.is_empty(),
        "placeholders not accounted for:\n{}",
        bad.join("\n")
    );
}
