# UniSQL roadmap

Written 2026-10-14 at origin/main 0844289 (0.2.0); updated at e68dc8f (0.3.0) and again after row locks. It records what is *not* done,
in the order the maintainers intend to take it, with the reason and how each item would be verified. It is
not a promise of dates, and the order may change. Direction comes from `docs/DESIGN.md` (sections
0.1, 2.1, 2.2, 5): comprehensive dialect coverage rather than
consumer-driven, dialects modular, the parser replaceable, SQLGlot the primary reference.

Every item ships with a differential test against the real engines (digest-pinned in
`.github/workflows/ci.yml`), a matrix row (leaf id plus contract line in `docs/LEAF-CONTRACTS.tsv`),
and a CHANGELOG entry; a form with no verified mapping is refused, never guessed.

## Next

1. **Corpus-driven reader gaps.** Re-run at the refs the D files were made at (humboldt `b7cd95e7`,
   auth `4582a64f`, lightbulb `34e5432b`) with the `note` field, the `not_in_ir` residual of DML is:
   `Interval` 21 (**done at 0.5.0**: `ts +/- INTERVAL 'n unit'` reads as `make_interval`), `current_date`
   without parentheses 7 (**done**) and `current_user` 1 (**done** as a refusal leaf), `Insert` as a set expression (`WITH .. INSERT`) 6 (**done**),
   `AtTimeZone` 2, `EscapedStringLiteral` 2, a function over a subquery 2 (these three **done**), `Extract` 1, `Array` 1.
   (`data_type` 25 are PostgreSQL catalog types, refused correctly; `statement` 65 and
   `multiple_statements` 34 are DDL and scripts, out of scope.) The PostgreSQL-source DML corpus has no residual the IR cannot read (`current_user`,
   `Extract` and `Array` are read and refused by leaf). The MySQL reader gaps found earlier
   (`<=>`, `INSERT IGNORE`, `||` as OR, `SUBSTRING`) were **done at 0.10.0**. The MySQL-source
   corpus now exists (`crates/unisql-differential/corpus/mysql/`, test `mysql_corpus`); its gap list
   is the next section.
   *Verified by:* the same corpus re-run: `not_in_ir` falls, and each new form has a differential case.
2. **Schema, what is left.** `Schema`, `schema_from_ddl` and collation interpretation are in. The live
   catalog reader is PersistAnt's, in its sqlx driver crate, after its D2-D6. Possible extensions: view
   columns from `CREATE VIEW`, `LIKE`/`ILIKE` against a column's collation (a different allowance than
   the one used for comparisons today).

## Later

- **Date and time functions** (`date_trunc`, `to_char`, `extract`, `age`, `date_part`): rare in the
  corpus (one `date_trunc`), kept for comprehensiveness (DESIGN.md section 0.1). Start from what the
  three engines agree on and refuse the rest by leaf (`fn:date_trunc.week`), not the whole function.
- **More verified mappings**, derived from SQLGlot only with a file+SHA trace and its MIT notice kept
  (`docs/LICENCES.md`): string and regexp functions, JSON paths for MySQL (`json_get_text` is refused
  there today), `substring`/`position` families.
- **Source dialects beyond the four** (SQL Server, Oracle), once target coverage for them is planned:
  the dialect crates are separable, so each is a new crate.
- **The adapter's parser dependency** stays replaceable: a second adapter (or an own reader for one
  dialect) is the test that the seam is real. Not started.
- **`unisql-differential` `corpus` runner as a CI artefact**, so the corpus percentages in the docs
  are regenerated, not hand-copied.

## Not planned here

- **DDL translation.** Decided with PersistAnt (D6): schema management stays with the migration tool.
- **Executing statements.** UniSQL returns text and bindings; drivers are the caller's.
- **Publishing**: 0.16.3 is on crates.io; each later version is an ordinary bump
  (`docs/PUBLISHING.md`).

## MySQL-source corpus: the gap list

Measured by `cargo test -p unisql-differential --test mysql_corpus` (verdicts in
`corpus/mysql/expected.tsv`, regenerated with `UNISQL_BLESS=1`). The corpus is 129 statements: 111
curated MySQL/MariaDB idioms (written from the reference manuals, not from any project; **their
counts say what is reachable, not how common it is**) and 18 DML statements of auth-framework's
MySQL storage backend (read at `881c696d`, real code). Every statement is accepted by MySQL 8.4 or
MariaDB 11.4 (`PREPARE` over the corpus schema); the translation to PostgreSQL, SQLite and MariaDB
runs with all allowances and the schema read from the corpus DDL, and each translated text is
prepared on the PostgreSQL / SQLite engine. What it does *not* do: execute and compare results (that
stays the per-mapping differential cases), and it is only as wide as the idioms listed.

At the first run: 129 statements; to PostgreSQL 57 translated or conditional, 27 refused by leaf,
41 not in the IR, 4 syntax errors (SQLite and MariaDB differ by a few). It found four defects, **fixed**
with the corpus: `schema_from_ddl` panicked on any quoted identifier in DDL; `UPDATE LOW_PRIORITY t`
was written as a table named `low_priority`; `DEFAULT` in `VALUES` was written as the identifier
`"default"`; SQLite `group_concat(DISTINCT x, ',')` is invalid (now `group_concat(DISTINCT x)`, or a
refusal for another separator).

Gaps, by what closing them takes (statement ids in the corpus in parentheses):

- **Reader, done at 0.17.5:** `INSERT .. SET` (a reader desugar to `(cols) VALUES (..)`).
- **Reader, done at 0.18.0 (IR fields, an AST break):** `REPLACE INTO` (native on MySQL/MariaDB and as
  `INSERT OR REPLACE` on SQLite; on PostgreSQL an upsert over every column when the schema states the
  table and its one unique key, allowance `replace_is_upsert`), `JOIN .. USING` and `NATURAL JOIN`.
- **Reader, done at 0.19.0 (IR field `Expr::IsBool`, an AST break):** `IS [NOT] TRUE` / `IS [NOT] FALSE` (`is_true`;
  translated to all three targets, differential cases against PostgreSQL, SQLite, MySQL, MariaDB).
- **Reader, done (IR fields `order_by`/`limit` on `Update`/`Delete`, an AST break):** `UPDATE/DELETE ..
  ORDER BY .. LIMIT` (`update_limit`, `delete_limit`; native on MySQL/MariaDB, a `ctid`/`rowid` subquery
  on PostgreSQL/SQLite).
- **Reader (not in the IR), would be mappable:** multi-table `UPDATE .. JOIN` / `DELETE t FROM .. JOIN` (`update_join`, `delete_join`);
  `DEFAULT` as a value (`insert_default`); `INSERT .. AS alias ON DUPLICATE KEY UPDATE a = alias.x`
  (`upsert_alias`, MySQL 8.0.19+ only); `LIKE BINARY` (sqlparser reads it as a cast to BINARY,
  `not_in_ir:data_type`), unary `~` (unsigned on
  MySQL), `CONVERT(x USING cs)` and `CAST(x AS CHAR(n))`/`BINARY`, `WITH ROLLUP`, three-argument `LOCATE`, `ROUND`/`POWER`/`SQRT`/`GREATEST`/`LEAST` (types or NULL handling differ), bit literals (hex literals are read). `DATE_ADD`/`DATE_SUB(x, INTERVAL n unit)` are **done** (read as `x +/- INTERVAL`).
- **Reader, parser-bound (sqlparser 0.63.0 rejects the text):** `7 DIV 2`, `LOCK IN SHARE MODE`,
  `SELECT .. INTO OUTFILE`, `TABLE t`. Upstream issues to file if a consumer needs them.
- **Reader, dialect-specific and not worth an IR form:** double-quoted strings (`"alice"`, a MySQL
  default that conflicts with SQL's identifier quoting: needs `ANSI_QUOTES` known), `STRAIGHT_JOIN`,
  `SQL_CALC_FOUND_ROWS`, index hints, user variables (`@x := ..`), `MATCH .. AGAINST`, `VALUES ROW(..)`
  with a column alias list, `LOW_PRIORITY`/`IGNORE`/`QUICK` modifiers.
- **Statements that are not DML** (`SET`, `SHOW`, `EXPLAIN`, `USE`, transactions, `CALL`,
  `TRUNCATE`): out of scope by design (translation is of DML; DDL and session control stay with the
  caller).
- **Target mappings: date/time group done at 0.17.4** (each with differential cases against the
  real engines): `UTC_TIMESTAMP()` / `UTC_TIMESTAMP(6)` (other precisions refused by leaf
  `fn:utc_timestamp.precision`; on SQLite text with millisecond resolution, allowance
  `timestamp_millisecond_resolution`), `UTC_DATE()`, `CURDATE()` (read as `CURRENT_DATE`),
  `DATEDIFF(a, b)`, `DATE(x)`; a MySQL/MariaDB source passes these through to MySQL/MariaDB unchanged.
  Auth-framework's two `UTC_TIMESTAMP(6)` statements now translate. **String group done at 0.18.2** (`CONCAT_WS`, `LEFT`, `RIGHT`, `REPEAT`, and on
  PostgreSQL `LPAD`/`RPAD`/`REVERSE`; SQLite has no `LPAD`/`RPAD`/`REVERSE`). **Still refused by leaf** (a
  verified mapping would close each): `DATE_FORMAT` (format-string translation),
  `UNIX_TIMESTAMP`/`FROM_UNIXTIME` (session time zone), `TIMESTAMPDIFF`, `YEAR`/`MONTH`/`DAY`/`HOUR`
  (map to `EXTRACT`, whose result type differs: numeric on PostgreSQL), `MD5`/`SHA1`/`SHA2`, `FIELD`, `RAND`, `UUID` (v1, not v4),
  `LAST_INSERT_ID`/`ROW_COUNT`/`VERSION`/`SLEEP`, the JSON function family.
- **`ON DUPLICATE KEY UPDATE` to PostgreSQL/SQLite: done at 0.17.3.** `Schema` carries unique keys
  (`Schema::with_unique_key`, or read by `schema_from_ddl` from `PRIMARY KEY`, `UNIQUE`, `CREATE
  UNIQUE INDEX`); a statement with no stated key is written `ON CONFLICT (key)` when the table has
  exactly one stated key (leaf `upsert.key_from_schema`), and still refused (`upsert.no_conflict_key`,
  reason names the schema) for several keys, none, or an index the reader could not express. All 3
  auth-framework upserts translate and are accepted by PostgreSQL and SQLite. Unqualified columns in
  the SET list are written `table.col` (PostgreSQL finds them ambiguous with `EXCLUDED`).
- **`text_collation` on almost everything:** the schema's string columns carry
  `utf8mb4_unicode_ci` (stated), so every comparison needs the allowance by design.

## Integration proposal: auth-framework's storage SQL through UniSQL

Proposal only, no code. Written 2026-10-14 at unisql 0.18.2 from auth-framework `origin/main`
`86de4cdf` (read with `git show`, never a working tree). The numbers below are from the `corpus`
runner over the SQL string literals of the two files named, with every allowance granted and no
schema; they are a classification of the text, not an execution.

**What is there today.** auth-framework has no `unisql` dependency. Its MySQL backend is gone from
`main` (removed because `sqlx-mysql` pulls `rsa`, RUSTSEC-2023-0071; its Cargo.toml says so), so the
statements this repository's MySQL-source corpus was cut from (`881c696d`, `src/storage/mysql.rs`) no
longer ship. Two SQL backends remain, each hand-written: `src/storage/postgres.rs` (sqlx, 29 literals:
20 DML, 9 DDL) and `auth-storage-sqlite/src/lib.rs` (unpublished crate, 28 literals: 22 DML, 6 DDL).
Migrations (DDL) live in `src/migrations`.

**What UniSQL does with them.**

| Direction | DML statements | Result |
|---|---|---|
| PostgreSQL text -> SQLite | 20 | all 20 translate: 11 exact, 9 conditional |
| SQLite text -> PostgreSQL | 22 | all 22 translate: 17 exact, 5 conditional (`text_collation`) |
| PostgreSQL text -> MariaDB/MySQL | 20 | 19 translate; 1 refused: `ON CONFLICT .. DO UPDATE .. WHERE` (no WHERE on `ON DUPLICATE KEY UPDATE`; line 410 of `postgres.rs`) |

Allowances the PostgreSQL -> SQLite set needs: `now_as_text` (9), `text_collation` (9),
`epoch_whole_seconds` (1), `lenient_cast` (1), `like_case_sensitivity_is_connection_setting` (1).
Each is a statement about their data that they can read and grant or not; `text_collation` shrinks
when a `Schema` read from their migrations says which columns are not text.

**What it would take.**

1. **One source of truth.** Keep the PostgreSQL statements (the recommended backend) as the text they
   maintain; derive the SQLite statements with `unisql::translate(sql, "postgres", "sqlite", &opts)`
   once at storage construction and cache the `Translation` (text, `ParamBinding`s). The second
   hand-written copy (`auth-storage-sqlite`) becomes generated, which is where the drift risk is.
2. **A reviewed allowance list** in one place in their crate, with a comment per allowance saying why
   it holds for their data. Anything not granted fails at startup naming the construct, not at query
   time.
3. **A schema.** `schema_from_ddl(<migrations>, "postgres")` into `Options::with_schema`, so non-text
   columns need no collation assumption and `ON CONFLICT`/upsert keys are known.
4. **Parameter binding.** Placeholders are renumbered by the translator; an array parameter
   (`= ANY($1)`) has a per-target binding in the report (`ParamBinding`) that the driver glue must
   follow (a JSON text on SQLite).
5. **Their tests do the proof.** Their existing storage tests run against both engines; the
   `unisql-differential` pattern (run the source text on one engine, the translation on the other,
   compare rows) is the model if they want the proof for each statement rather than a spot check.
6. **Dependencies and versions.** `unisql = "0.18"` from crates.io once it is published
   (0.x: the second number is the breaking one, so pin the minor); its MSRV (1.94) is below theirs
   (1.95). No driver is pulled in.

**What stays theirs.** DDL and migrations (UniSQL does not translate DDL), drivers, row mapping, and
any statement outside the DML the corpus covers (`CREATE TABLE`, `CREATE INDEX`, 15 of the 57 literals).

**MySQL/MariaDB again.** UniSQL would produce MySQL text for 19 of 20 statements, but the blocker that
removed the backend was the driver crate's dependency, not the SQL: a MySQL driver without that
advisory, or an accepted risk, is their decision; the refused statement needs a rewrite of the
`WHERE` into the SET expressions.

**Risks.** Allowances are assumptions, so a wrong one is a silent behavioural difference: that is why
each is named and why they should be granted one by one. UniSQL is pre-1.0 and has no consumer yet;
auth-framework would be the first, so an API change in either direction is cheap now and expensive
later.

## Known gaps (documented, not planned)

- **`ON CONFLICT (cols) WHERE <index predicate> DO ...`** (a partial unique index as the arbiter):
  `sqlparser` 0.63.0 rejects the predicate in the conflict target, in the PostgreSQL and generic dialects,
  so the reader reports a syntax error for it (one Humboldt statement). Upstream:
  [apache/datafusion-sqlparser-rs#2625](https://github.com/apache/datafusion-sqlparser-rs/issues/2625)
  (open) and its fix [#2627](https://github.com/apache/datafusion-sqlparser-rs/pull/2627) (open PR).
  Once released, bump the pin, add `OnConflict.target_where` to the IR (PostgreSQL and SQLite write it;
  MySQL refuses it by leaf) and a differential case. Nothing to do on our side until then.
- **`INSERT OR REPLACE`** (SQLite replace semantics with no conflict key): not in the IR.
- **Table-valued functions in FROM** are read and refused by leaf (`table_function.<name>`) on SQLite and
  MySQL/MariaDB; `generate_series` could be mapped through a recursive CTE once a consumer needs it.
