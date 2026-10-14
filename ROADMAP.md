# UniSQL roadmap

Written 2026-10-14 by the unisql lane at origin/main 0844289 (0.2.0); updated at e68dc8f (0.3.0) and again after row locks. It records what is *not* done,
in the order the lane intends to take it, with the reason and how each item would be verified. It is
not a promise of dates, and the PM may reorder it. Direction comes from `docs/DESIGN.md` (sections
0.1, 2.1, 2.2, 5 carry the user's words verbatim): comprehensive dialect coverage rather than
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
- **Publishing is the PM's call**: 0.16.3 is on crates.io; each later version is an ordinary bump
  (`docs/PUBLISHING.md`). The repository's public flip is CireSnave's.

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
- **Reader (not in the IR), would be mappable:** `UPDATE/DELETE .. ORDER BY .. LIMIT` (`update_limit`,
  `delete_limit`: PostgreSQL has no direct form, needs a key subquery, SQLite needs a compile
  option); multi-table `UPDATE .. JOIN` / `DELETE t FROM .. JOIN` (`update_join`, `delete_join`);
  `DEFAULT` as a value (`insert_default`); `INSERT .. AS alias ON DUPLICATE KEY UPDATE a = alias.x`
  (`upsert_alias`, MySQL 8.0.19+ only); `LIKE BINARY`, `REGEXP`/`RLIKE`, `IS TRUE`, `XOR`, bitwise
  operators, `CAST(x AS CHAR)`/`CONVERT`, `WITH ROLLUP`, `LOCATE`/`INSTR`/`POSITION`, `CEIL`, hex and
  bit literals, `DATE_ADD(x, INTERVAL n unit)` (read via the `+ INTERVAL` path only).
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
  Auth-framework's two `UTC_TIMESTAMP(6)` statements now translate. **Still refused by leaf** (a
  verified mapping would close each): `DATE_FORMAT` (format-string translation),
  `UNIX_TIMESTAMP`/`FROM_UNIXTIME` (session time zone), `TIMESTAMPDIFF`, `YEAR`/`MONTH`/`DAY`/`HOUR`
  (map to `EXTRACT`, whose result type differs: numeric on PostgreSQL), `CONCAT_WS`,
  `LEFT`/`RIGHT`/`LPAD`/`REPEAT`/`REVERSE`, `MD5`/`SHA1`/`SHA2`, `FIELD`, `RAND`, `UUID` (v1, not v4),
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
