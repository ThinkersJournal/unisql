# Changelog

One version number for every crate in the workspace. Pre-1.0, the second number is the breaking one.

## Unreleased (version allocated by the PM at gate)

- **`expr AT TIME ZONE zone`** and **`ARRAY(SELECT ..)`** are in the IR (`Expr::AtTimeZone`,
  `Expr::ArraySubquery`) with leaves `at_time_zone` and `array_subquery`: PostgreSQL writes them back
  (round trip tested on the engine), SQLite and MySQL/MariaDB refuse them by leaf instead of failing the
  read. **`E'..'` strings** read as the string they denote (the parser resolves the escapes).
- Corpus effect: 2 `AtTimeZone`, 2 `function_subquery` (`ARRAY(SELECT ..)`) and 2 `EscapedStringLiteral`
  rows are no longer `not_in_ir`. The statements that hold them (`to_char(..)`, `r::text`, `md5(..)`)
  stay untranslatable for those other reasons: this is classification, not new coverage.
- `Expr` and `Target` gain variants/hooks (`at_time_zone`, `array_subquery`): breaking for consumers
  of `unisql-core::ast`.

## 0.8.0 - 2026-10-14

- **`WITH` before a modifying statement and data-modifying CTEs** (the 6 `not_in_ir set_expr Insert` rows
  of the corpus): `WITH .. INSERT | UPDATE | DELETE ..` and `WITH x AS (INSERT/UPDATE/DELETE ..
  RETURNING ..) ..` are in the IR (`Statement::With`, `CteBody::Dml`). Leaves `with.insert`,
  `with.update`, `with.delete`, `cte.data_modifying`. Probed on the pinned engines: PostgreSQL has all;
  SQLite has `WITH` before all three but no data-modifying CTE; MySQL 8.4 has `WITH` before UPDATE and
  DELETE only; MariaDB 11.4 has none of the four. Each refusal is its leaf, never a guess.
- **Breaking for consumers of `unisql-core::ast`:** `Cte::query` became `Cte::body: CteBody`, and
  `Statement` and `Target` gained a variant / hooks (`Statement::With`, `with_statement`,
  `data_modifying_cte`).
- Differential: a new `Expect::Identity` (source dialect round-trip, rows and state compared) for forms
  no target but the source has; cases for every engine.

## 0.7.0 - 2026-10-14

- **Bare `CURRENT_DATE`** (7 `not_in_ir bare_function` rows of the corpus, all INSERT values) is read as
  `fn:current_date` and written as `CURRENT_DATE` on PostgreSQL and MySQL/MariaDB (a date in the session
  time zone: exact) and on SQLite as `CURRENT_DATE` with the new allowance `date_as_text` (text
  `YYYY-MM-DD`, the UTC date). `CURRENT_DATE()` with parentheses (MySQL) reads the same.
  **Breaking:** the `Allowance` enum gains the variant `DateAsText` and is now `#[non_exhaustive]` (taken
  once, so the next variant is not breaking): a consumer matching it must add a wildcard arm.
  Leaves: `fn:current_date`, `fn:current_date.date_as_text`; contract in `docs/LEAF-CONTRACTS.tsv`.
- Differential vs real engines: a `CAST(CURRENT_DATE AS TEXT/CHAR)` case in each direction plus the
  allowance gate on SQLite sources and targets.

## 0.6.0 - 2026-10-14

- **Gate too strict (fixed):** a statement that translates now has every leaf `analyse` names for it
  recorded in the report. Writers note only divergences, so leaves a target accepts without one
  (SQLite's bytewise `text_comparison`, `concat`, `division_integer`, `returning.update`, MySQL's
  `ordered_comparison`, `fn:make_interval` in interval arithmetic, ...) had no matrix row and a gate
  requiring them refused what the translator gives. They are `enabled=yes` rows now (new rows; no
  existing row changes value). Test first: `every_need_of_a_translated_statement_is_recorded_for_that_target`
  failed on the 25 statement/direction pairs below before the change (measured at main 229a859).
- **Breaking: a gate verdict flips from refused to accepted** for these 25 (statement, direction) pairs,
  all of which the translator already translated. Leaves that had no row and now have `enabled=yes`:
  - postgres -> sqlite, `text_comparison` (6 statements): `SELECT id, name FROM users WHERE id = $1`;
    `... WHERE age > $1 AND city = 'Oslo' ORDER BY age DESC NULLS LAST LIMIT 5 OFFSET 2`;
    `SELECT u.id, o.id FROM users u LEFT JOIN orders o ON o.user_id = u.id`;
    `UPDATE users SET age = age + 1 WHERE city = 'Oslo'`;
    `SELECT count(*) FILTER (WHERE status = 'paid') FROM orders`;
    `... WHERE active = TRUE AND age BETWEEN 20 AND 30`.
  - postgres -> sqlite: `concat` and `division_integer` (`SELECT CAST(id AS TEXT), now(), 7 / 2, 'a' || name
    FROM users`); `fn:make_interval` and `named_arg` (`... WHERE at > now() - make_interval(secs => $1)`);
    `returning.update` + `text_comparison` (`UPDATE users SET age = 1 WHERE id = $1 RETURNING id`);
    `trim.multi_char` (`SELECT trim(both 'xy' from name) FROM users`).
  - postgres -> mysql and postgres -> mariadb (each of the 6 statements, so 12 pairs),
    `ordered_comparison`: `... WHERE age > $1 AND city = 'Oslo' ORDER BY ...`; `DELETE FROM orders WHERE amount < 100`;
    `WITH RECURSIVE t(n) AS (...) SELECT n FROM t`; `... WHERE active = TRUE AND age BETWEEN 20 AND 30`;
    `SELECT id FROM users WHERE age > 3 FOR UPDATE SKIP LOCKED`; and, with `fn:make_interval` and
    `named_arg`, the `make_interval(secs => $1)` statement.
  - mysql -> postgres, mysql -> sqlite, mariadb -> postgres, `concat`: `SELECT concat(name, '-',
    ifnull(city, '?')), 7 / 2, 7 DIV 2, length(name) FROM users LIMIT 2, 3`.
  No row that existed changed its value.
- `make_interval(..)` outside `ts +/- ..` is its own leaf `fn:make_interval.standalone` (refused by
  every target without an interval type); `named_arg` is no longer a need of the interval arithmetic form
  (its notation is not an output on SQLite/MySQL).

## 0.5.0 - 2026-10-14

- **`INTERVAL` literals:** `ts +/- INTERVAL '5 minutes'` (PostgreSQL) and `ts +/- INTERVAL 5 MINUTE`
  (MySQL) read as `make_interval(mins => 5)`, so every target's interval arithmetic applies (SQLite
  text arithmetic, MySQL `INTERVAL`, PostgreSQL native). 21 of the 25 `not_in_ir expression` rows of
  the corpus. Only a single `<number> <unit>` is read; compound and range intervals stay not-in-IR.
- **Behaviour change of an existing leaf (why 0.5.0):** `fn:make_interval` on the PostgreSQL target from a
  MySQL/SQLite source was refused and is now accepted for the fixed-length units (secs, mins, hours,
  days, weeks); months/years stay refused as `fn:make_interval.month_year`.
- Months and years are their own leaf `fn:make_interval.month_year` (refused by all targets from a
  foreign source), so `fn:make_interval` is no longer both proven and refused in one matrix file.
- Differential: `events` fixtures in the MySQL-source suites; cases for each spelling.

## 0.4.1 - 2026-10-14

- ROADMAP corrected from the corpus counts: date/time functions are one statement of the corpus, not
  the largest residual (`not_in_ir expression` 25 and `data_type` 25 are). The `corpus` runner now
  emits a `note` field with the reader's detail for `not_in_ir` rows. No translation change.

## 0.4.0 - 2026-10-14

- **Row locks:** `FOR UPDATE | FOR SHARE [OF table] [NOWAIT | SKIP LOCKED]` are in the IR
  (`RowLockClause`). Leaves: `row_lock` (FOR UPDATE), `row_lock.share`, `row_lock.of`,
  `row_lock.nowait`, `row_lock.skip_locked`. Measured against the engines with a second session holding
  a row: NOWAIT raises and SKIP LOCKED skips on PostgreSQL, MySQL 8.4 and MariaDB 11.4. MySQL has all
  forms; MariaDB 11.4 has no `FOR SHARE` and no `FOR UPDATE OF` (syntax errors, probed) and refuses them;
  SQLite refuses every form (as before).
- **Fix:** the locking clause is printed after ORDER BY / LIMIT / OFFSET. It was printed before them,
  so `... ORDER BY x FOR UPDATE` translated to a syntax error on every target.
- Harness: a case can hold a lock in a second session (`Case::hold`) and expect an error on both
  engines (`Expect::BothError`).
- `unisql::analyse` names the new lock leaves; `docs/LEAF-CONTRACTS.tsv` has their contracts.

## 0.3.0 - 2026-10-14

- **MySQL/MariaDB `LIKE`/`ILIKE`:** the cause is the column collation, not a connection setting (that is
  SQLite). `like` now carries `text_collation`, so it is read from `config` like the other text leaves:
  strict `like` is `yes` under `collation=utf8mb4_bin`, with a known-bad control (default collation,
  must diverge). `ilike` carries `text_collation` and `ascii_only_case_folding` (verified: PostgreSQL's
  `'É' ILIKE 'é'` is false in the C locale, MySQL's `LOWER()` form is true). The leaf
  `like.like_case_sensitivity_is_connection_setting` no longer exists for the MySQL/MariaDB targets.

## 0.2.1 - 2026-10-14

- `ROADMAP.md` (unisql had none). The `Allowance` doc comment now says a variant's name and text are a
  contract: rewording is a new variant, not an edit (PersistAnt request). No behaviour change.

## 0.2.0 - 2026-10-14

- **Breaking (ids, matrix files, `analyse`):** constructs are now leaf ids. `cast` becomes
  `cast.to_<type>`; partial forms get their own leaf (`upsert.no_conflict_key`, `returning.update`,
  `trim.multi_char`, `fn:<agg>.order_by`, `distinct_on.star_projection`, `distinct_on.row_lock`,
  `distinct_on.set_operation`). `Untranslatable::subject()` returns the leaf; the `.refused` suffix
  is gone, a refusal is `enabled=no, reason=missing`. `analyse().constructs` names every form a
  statement needs, refused ones included (tested for every target).
- Matrix rows: a divergence a caller may accept is its own leaf `<construct>.<allowance>`; the strict
  leaf is `no/wrong`. Text collation is read from `config`: `yes` under the fixture collation, with a
  known-bad control (default MySQL/MariaDB `utf8mb4` collation, must diverge) recorded as `no/wrong`.
  `metadata` carries `suite` and `test`.
- `docs/LEAF-CONTRACTS.tsv` with a test that keeps it in step with the matrix files.

## 0.1.0 - 2026-10-14

- **Breaking (matrix files):** rows follow PersistAnt's merged row schema (DESIGN-KINDS.md section
  2.4 at 4fed258e): `how`/`evidence` are replaced by `config`, `enabled` (yes|no), `reason` and
  `metadata` (`tier=..;suite=..`). A refusal is `enabled=no, reason=missing`. Leaf ids and the MySQL
  default-collation known-bad run are not part of this change (still waiting on PersistAnt D2-D6).

## 0.0.3 - 2026-10-13

- `unisql::analyse(sql, source)`: target-free needs extraction (`constructs`, `refused`, `unreadable`),
  backed by a new IR walker in `unisql-core` (`needs::constructs`) and `Allowance::ALL`. Requested by
  the PersistAnt lane for deriving a program's needs from its statement catalogue.

## 0.0.2

- Lane `HANDOFF.md` is no longer tracked and is ignored (hygiene; no code change).

## 0.0.1

- First tracked state of UniSQL: IR, PostgreSQL/SQLite/MySQL/MariaDB readers and writers, parameter
  codec for array parameters, differential suites against real engines, matrix files, first CI.
