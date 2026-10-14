# Changelog

One version number for every crate in the workspace. Pre-1.0, the second number is the breaking one.

## 0.17.1 - 2026-10-14

- Docs only: ROADMAP item 1 no longer points at a list that does not exist; no code change.

## 0.17.0 - 2026-10-14

- **Table-valued functions in FROM** (`FROM generate_series(1, 3) AS g`, `FROM t, jsonb_object_keys(doc)
  AS k`; 4 corpus statements, until now `not_in_ir`) are in the IR (`TableRef::Function`, breaking for
  consumers of `unisql-core::ast`): PostgreSQL writes them (round trip on the engine); SQLite and
  MySQL/MariaDB refuse them by leaf `table_function.<name>`. Classification only: no mapping.
- Known gap documented (ROADMAP): the partial-index conflict target `ON CONFLICT (cols) WHERE ..` is a
  `sqlparser` limit with an upstream issue (#2625) and fix PR (#2627), both open.
- README/PUBLISHING/ROADMAP say the six crates are published (0.16.3).

## 0.16.3 - 2026-10-14

Release preparation; no code or behaviour change. **Nothing is published.**

- `publish = true` in `[workspace.package]` (the six crates inherit it); `unisql-differential` states
  `publish = false` itself.
- The leaf-contract and licence-copy checks moved from the `unisql` package's tests to the unpublished
  `unisql-differential` crate (`tests/leaf_contracts.rs`, run by its own CI step, no engine), so the
  `unisql` package has no excluded test and `cargo publish --workspace --dry-run --locked` is
  warning-free.
- README status paragraph and `docs/PUBLISHING.md` updated.

## 0.16.2 - 2026-10-14

- `docs/PUBLISHING.md`: the clean-archive `cargo publish --workspace --dry-run --locked` result, and a
  correction: `publish = false` in `[workspace.package]` must be set to `true`, not deleted. Docs only.

## 0.16.1 - 2026-10-14

Packaging only; no code or behaviour change.

- **Publish checklist** `docs/PUBLISHING.md`: names (all six held by `ciresnave` at 0.0.0; `unisql-
  differential` unclaimed and stays unpublished), what a release changes, package contents, order,
  docs.rs, provenance, open points. Nothing is published.
- **Findings fixed:** `description` on `unisql-core`, `unisql-dialect-postgres`, `unisql-dialect-sqlite`
  (crates.io requires one); `LICENSE-MIT` and `LICENSE-APACHE` and a `README.md` inside every package
  (a test keeps the licence copies identical to the root's); `tests/leaf_contracts.rs` (reads the
  workspace layout) is excluded from the `unisql` package; `keywords` and `categories`.
- `README.md` no longer says "design phase, no crate name reserved"; its example uses the leaf ids.

## 0.16.0 - 2026-10-14

PM task part (b): sweep the statements of auth-framework (`origin/main` 1b31e902) and lightbulb
(`origin/main` c5b7fd77) for gaps, read through `git show`, never a working tree.

- **Sweep result** (the PostgreSQL-source DML literals of both repos: 36 + 12 statements, all allowances
  granted): the `?`-placeholder statements of `auth-framework/src/authorization_enhanced/storage.rs` were
  read as PostgreSQL by the old runner (a runner limit, not a gap: they are SQLite/MySQL text). With
  `UNISQL_SOURCE=sqlite|mysql` (new, `corpus` runner) 11 of 12 translate; the 12th is `INSERT OR REPLACE`
  (SQLite replace semantics, no conflict key: not in the IR). Of the PostgreSQL-source statements the
  real gaps were `date_trunc`, `to_timestamp`, `EXTRACT(EPOCH FROM a - b)` and `::timestamp` on SQLite;
  all four are mapped below. Left, with reasons: `AT TIME ZONE` (no verified zone mapping),
  `ON CONFLICT .. DO UPDATE .. WHERE` on MySQL (ODKU has no WHERE; emulating it with IF() depends on
  assignment order), `::inet` (a PostgreSQL network type: not in the IR).
- **`date_trunc('year'|'month'|'day'|'hour'|'minute', ts)`** on MySQL/MariaDB (`DATE_FORMAT` then
  `CAST AS DATETIME`, exact) and SQLite (`strftime`, text, `timestamp_as_text`); one leaf per field
  (`fn:date_trunc.<field>`), other fields stay refused.
- **`to_timestamp(seconds)`**: `FROM_UNIXTIME` on MySQL/MariaDB (exact, fraction kept); `datetime(x,
  'unixepoch')` on SQLite (text, whole seconds: `timestamp_as_text` + the new `timestamp_whole_seconds`).
  The two-argument formatter form is not mapped.
- **`EXTRACT(EPOCH FROM a - b)`** (`extract.epoch.interval`): `TIMESTAMPDIFF(MICROSECOND ..) / 1000000`
  on MySQL/MariaDB (exact, fraction kept); whole seconds between the two timestamp texts on SQLite
  (`epoch_whole_seconds`).
- **`CAST(x AS TIMESTAMP)`** on SQLite (it was refused): keeps the text (`timestamp_as_text`,
  `lenient_cast`).
- Each mapping is a differential case on the real engines against PostgreSQL's value on fixed
  timestamps; the refusals (`date_trunc('week')`, `EXTRACT(WEEK)`, `EXTRACT(DOW)`) are cases too.
- `corpus` runner: `UNISQL_SOURCE`; `Allowance::TimestampWholeSeconds` (non_exhaustive enum);
  `Target::epoch_of_difference` (default `None`).

## 0.15.0 - 2026-10-14

PM task: verify and enable one mapping per refused leaf, largest first. Each mapping below was run
against the real engines in the differential suites; one that could not be verified stays refused.

- **`gen_random_uuid()` on MySQL, MariaDB and SQLite** (the largest refused leaf of the corpus, 12
  statements): assembled from `RANDOM_BYTES` / `randomblob` as a real **version 4** UUID (version nibble
  4, variant nibble 8/9/a/b, uniform), tier **Emulated**, no allowance needed (it is not a v1 `UUID()`).
  The suite checks the property on every run (`Expect::RandomUuidV4`): 360 values per target, all v4
  shaped and all distinct, and a control (`UUID()` / un-hyphenated hex) must be rejected by the same check.
  Also checked outside the suite: 2000 values per engine.
- **`EXTRACT(year|month|day|hour|minute FROM ts)`** on MySQL/MariaDB (native `EXTRACT`, exact) and SQLite
  (`strftime`, conditional on the new allowance `timestamp_as_text`); **`EXTRACT(EPOCH FROM ts)`**
  (`UNIX_TIMESTAMP` / `strftime('%s')`, whole seconds: new allowance `epoch_whole_seconds`). Compared with
  PostgreSQL's values on fixed timestamps on the engines.
- **Breaking:** the epoch of a *difference* (an interval) is its own leaf `extract.epoch.interval` and stays
  refused; `extract.epoch` is now only the epoch of a timestamp. `Target::extract` gained an `interval`
  argument. Leaves that were refused are now enabled (`fn:gen_random_uuid`, `extract.year|month|day|hour|
  minute|epoch`): a gate verdict flips refused -> accepted for statements using them. Two new
  allowances (the enum is `#[non_exhaustive]`).
- **Not mapped, with the reason** (corpus refused list after this change, Humboldt src + tests):
  `row_lock` and `cte.data_modifying` on SQLite (the engine has neither), `fn:md5` on SQLite (no builtin),
  `cast.to_uuid` (no UUID type to validate against), `fn:to_regclass` / `fn:hashtext` / `fn:convert_to`
  (PostgreSQL internals), `fn:current_database` (MySQL's `DATABASE()` is a schema, not a PostgreSQL
  database: the meaning differs, so not claimed), `fn:people_backfill` (a project function).

## 0.14.0 - 2026-10-14

- **The last three DML forms the Humboldt/auth corpus could not read** (one statement each):
  `ARRAY[a, b]` (`Expr::ArrayLiteral`, leaf `array_literal`), `EXTRACT(field FROM x)` (`Expr::Extract`,
  one leaf per field: `extract.epoch`, `extract.year`, ...) and the bare identity keywords `current_user`,
  `session_user`, `current_role` (`fn:current_user`, ...). PostgreSQL writes each back (round trip on the
  engine); SQLite and MySQL/MariaDB refuse each by leaf. A field is mapped on a target only once it is
  verified, so enabling one later changes one leaf, not all of EXTRACT.
- `Expr` and `Target` gain variants/hooks (`extract`, `array_literal`): breaking for consumers of
  `unisql-core::ast`.

## 0.13.0 - 2026-10-14

- **Column collation is interpreted** (the `Schema` field was only carried until now). A comparison
  (`=`, `<>`, `<`, `<=`, `>`, `>=`, `BETWEEN`) or an `ORDER BY` on a text column whose stated collation is
  not bytewise (`en_US`, `NOCASE`, `utf8mb4_0900_ai_ci`, ...), translated to another dialect, now needs the
  new allowance `source_collation_not_bytewise`: the target compares bytes, so the result may differ for
  the same data. Leaves: `text_comparison` / `order_by` with `.source_collation_not_bytewise`.
  `is_bytewise_collation(name)`: `C`, `POSIX`, `ucs_basic`, `BINARY`, `*_bin`.
- A bytewise stated collation changes nothing (the target half of `text_collation` is still the caller's
  or the gate's); a column with **no** stated collation stays unknown, never "the database default"
  (PersistAnt's ask). The same dialect in and out is the identity: nothing to accept.
- **Behaviour change:** a statement over such a column that translated with `text_collation` alone now
  also needs `source_collation_not_bytewise` (only when a `Schema` with that column was passed).
- Fix: an `ORDER BY` is printed with its SELECT's tables in scope (it was resolved after the scope
  ended, so a schema never reached it).
- New `Allowance` variant (the enum is `#[non_exhaustive]`).

## 0.12.1 - 2026-10-14

- **`unisql::schema_from_ddl(ddl, dialect)`** (option B): fills a `Schema` from DDL text (a project's
  migrations, concatenated in order). Reads `CREATE TABLE`, `ALTER TABLE .. ADD | DROP | ALTER COLUMN ..
  TYPE` and `DROP TABLE`; reading only, nothing is translated or written. Statements split on `;`
  tokens, so a dollar-quoted function body does not split; a statement the parser cannot read is listed
  in `DdlSchema::skipped` (it can only leave columns unregistered). A column whose declared type is not
  classified (a custom type such as `citext`, an array) is registered as `ColumnClass::Unknown`, never
  omitted, so an unqualified reference cannot be taken for another table's column. `boolean` is
  `Boolean` only when declared so (MySQL `tinyint(1)` is `Numeric`). The collation is only what the
  column states (`COLLATE x`); no database default is guessed (PersistAnt's ask).
- `ColumnClass::Unknown`, `Schema::set_column / drop_column / drop_table`.
- **Measured on Humboldt `backend/migrations` + `backend/src` @ b7cd95e7** (365 statements, all
  allowances granted; the `corpus` runner takes the DDL as a second argument): on the MySQL target
  97 statements go from conditional to exact and the statements needing `text_collation` fall from 226
  to 108; on SQLite 1 changes (53 -> 47 needing it). 6 of the migrations' statements are skipped
  (`DO` blocks, one `COMMENT` the parser rejects).

## 0.12.0 - 2026-10-14

- **`Schema` (option A of the schema-input ruling):** `Options::with_schema(Schema)` tells the translator
  the class of base-table columns (`Text | Numeric | Boolean | Other`, plus an optional collation that is
  carried and not yet interpreted). A comparison with a column that is not text then involves no text
  collation: it no longer needs `text_collation`, and `analyse_with(sql, source, &opts)` names no
  `text_comparison` for it. Scope/alias resolution is inside UniSQL; the schema supplies base tables
  only. Names match exactly as the reader produces them; an unregistered table in scope, a derived
  table, a view or an alias makes an unqualified column unknown (conservative, never a guess).
  Tested on the real engines: numeric-column comparisons translate with no allowance and the results
  agree on SQLite, MySQL and MariaDB; a text column and the no-schema case still need `text_collation`.
- **Matrix files:** `config` now ends `;schema=none` (every row is computed without a Schema, and a case
  run with one is left out of the files): PersistAnt's request, so a `schema=none` row is never read as
  proven for a column map. **Breaking for a reader that matches `config` exactly.**
- `needs::constructs_with` and `unisql::analyse_with` are new; `Ctx` gains `scopes` (internal).

## 0.11.0 - 2026-10-14

- **Breaking, taken once:** `Options` is `#[non_exhaustive]`, so a field added later (the planned
  column-type `Schema`) is not breaking again. Build it with `Options::default().with(..)`; a struct
  literal `Options { allow }` outside `unisql-core` no longer compiles. Nothing else changes.

## 0.10.0 - 2026-10-14

- **MySQL reader gaps closed**, each tested differentially from a MySQL and a MariaDB source against
  PostgreSQL and SQLite, and each with the assumption it carries named:
  - `a <=> b` is `BinaryOp::NullSafeEq` (leaf `null_safe_eq`): `IS NOT DISTINCT FROM` on PostgreSQL,
    `IS` on SQLite, `<=>` on MySQL/MariaDB. Exact.
  - `INSERT IGNORE` reads as `ON CONFLICT DO NOTHING` (no target), allowance
    `ignore_covers_only_unique_conflicts` (leaf `insert_ignore`).
  - `||` is read as logical OR (MySQL's default `sql_mode`) by rewriting the operator token before
    parsing (the parser gives it concatenation's precedence), allowance `pipes_as_logical_or`
    (leaf `pipes_as_or`). A `||` inside a string or comment is untouched (tested).
  - `SUBSTRING`: exact for a literal start >= 1 and a literal length >= 0 (or none); any other start
    or length carries allowance `substring_start_assumed_positive` (leaf `fn:substr.start_position`).
    Measured: a start of 0 returns '' on MySQL and a prefix on PostgreSQL (a divergence case).
  - A MySQL/MariaDB source's own `SUBSTRING` passes through to a MySQL/MariaDB target unchanged.
- Three new `Allowance` variants (the enum is `#[non_exhaustive]` since 0.7.0) and `BinaryOp::NullSafeEq`.
- **Breaking:** `BinaryOp` is now `#[non_exhaustive]` (taken once, like `Allowance` at 0.7.0, so a later
  operator is not breaking again): a consumer matching it needs a wildcard arm.

## 0.9.0 - 2026-10-14

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
