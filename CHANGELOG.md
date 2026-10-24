# Changelog

One version number for every crate in the workspace. Pre-1.0, the second number is the breaking one.

## 0.26.5 - 2026-10-24

- **Docs: spec-to-roadmap audit.** `ROADMAP.md` gains a section listing the work the design documents name that no row covered, each marked open, held or done; stale 0.16.x and "not yet published" text in `README.md` and `ROADMAP.md` corrected. No code change.

## 0.26.4 - 2026-10-24

- **Internal: `check_dialect` in `unisql-macro-core` no longer returns a `bool` that no caller read.** No behaviour change.

## 0.26.3 - 2026-10-24

- **Lint: the two `allow(clippy::too_many_arguments)` attributes carry a `reason`.** No behaviour change.

## 0.26.2 - 2026-10-24

- **CI: advisory `portfolio` job** calls a pinned reusable workflow for an extra static-analysis pass. It does not gate merges. No code change.

## 0.26.1 - 2026-10-24

- **Docs: `assets/PROVENANCE.md`** records each logo file's hash, first-add commit, embedded metadata and licence status. The SVG is verified against Openclipart; the maker of the three PNGs is not established and is listed as an open question. No code change.

## 0.26.0 - 2026-10-24

- **Breaking: `LIKE` / `ILIKE` on a text column whose stated collation is not bytewise now needs `source_collation_not_bytewise`** when the target is another dialect, as `=`, `BETWEEN` and `ORDER BY` already did. Before, such a statement translated without naming the divergence: under a `_ci` or locale collation `name LIKE 'a%'` matches `Alice`, and the target's `LIKE` need not. A bytewise (`C`, `_bin`) or unstated collation, and a same-dialect translation, are unchanged.

## 0.25.10 - 2026-10-24

- **`schema_from_ddl` reads the columns of a view.** `CREATE [OR REPLACE] VIEW v [(a, b)] AS SELECT ..` registers `v` when every output column can be named (the view's own column list, an alias, or a bare column reference). A bare reference to a column of the query's one table keeps its class, collation and declared type, so a comparison on a text view column needs the collation allowance and one on a number does not; any other output column is registered as unknown; nullability and default are left unknown (a view's are not the table's). `SELECT *` expands over a single table with a complete column list. A set operation, a `WITH`, an unnamed expression or a wildcard it cannot expand leaves the view unregistered, and `DROP VIEW` removes it.

## 0.25.9 - 2026-10-23

- **Security: `ctutils` 0.4.3 to 0.4.5 in `Cargo.lock`** (RUSTSEC-2026-0385, fixed in 0.4.4). Lockfile only; no code change.

## 0.25.8 - 2026-10-23

- **SQLite `INTEGER PRIMARY KEY` is read as having a default** (it is the rowid: an `INSERT` may leave it out or give it `NULL`). Before, `schema_from_ddl(.., "sqlite")` reported it `NOT NULL` with no default, so a catalog check that "an `INSERT` leaves out a NOT NULL column" would have rejected valid SQLite statements. `BIGINT PRIMARY KEY`, `TEXT PRIMARY KEY` and PostgreSQL's `integer PRIMARY KEY` are unchanged.
- **`SchemaChecker` rejects a literal `NULL` for a column the schema states is `NOT NULL` with no default**: in `INSERT (cols) VALUES (.., NULL, ..)` (every row) and `UPDATE .. SET col = NULL`. A compile error in `sql!` (`Kind::Catalog`). Only when certain: an unstated flag, a column with a default, and a positional `INSERT` (the schema does not state declaration order) are not reported.

## 0.25.7 - 2026-10-23

- **`age(a, b)` is refused with a reason** (leaf `fn:age`) on SQLite, MySQL and MariaDB: it returns a calendar interval of years, months and days, which none of them can hold, and month-end rules differ. PostgreSQL to PostgreSQL is unchanged. This closes the date/time row (`date_trunc`, `EXTRACT`, `date_part`, `to_char` are translated; `age` is a typed refusal).

## 0.25.6 - 2026-10-23

- **`to_char(ts, fmt)` is translated to SQLite, MySQL and MariaDB for a literal format made of `YYYY`, `MM`, `DD`, `HH24`, `MI`, `SS` and the separators `- / : . ,` and space** (`DATE_FORMAT` on MySQL/MariaDB, Exact; `strftime` on SQLite, Conditional on `TimestampAsText`). Differentially verified against PostgreSQL 16. Any other format (`Day`, `Mon`, `FM..`, `AM`, numeric formats) stays refused with the leaf `fn:to_char.format`.

## 0.25.5 - 2026-10-23

- **PostgreSQL `date_part('field', x)` with a literal field is read as `EXTRACT(field FROM x)`** and so translates wherever that does (year, month, day, hour, minute, epoch, dow, isodow, doy, quarter, week). The value is the same; PostgreSQL's `EXTRACT` returns numeric where `date_part` returns double precision. A field that is not a literal stays the function and is refused with the leaf `fn:date_part`.

## 0.25.4 - 2026-10-22

- **`date_trunc('week' | 'quarter', ts)` is translated to SQLite, MySQL and MariaDB** (week: the Monday of the ISO week; both were refused). MySQL/MariaDB are Exact; SQLite is Conditional on `TimestampAsText`. Differentially verified against PostgreSQL 16 on year-boundary dates. Other fields (`decade`, ..) stay refused by leaf.

## 0.25.3 - 2026-10-22

- **Docs only.** `docs/PUBLISHING.md` records the 0.25.1 release of all nine crates (`cargo publish --workspace --locked` orders them) and the current dependency order including `unisql-macro-core`, `unisql-embed` and `unisql-macros`.

## 0.25.2 - 2026-10-22

- **`EXTRACT(DOW | ISODOW | DOY | QUARTER | WEEK FROM ts)` is translated to SQLite, MySQL and MariaDB** (was refused). MySQL/MariaDB use `DAYOFWEEK`, `WEEKDAY`, `DAYOFYEAR`, `QUARTER` and `WEEK(x, 3)` (Exact); SQLite derives them from the timestamp text (Conditional on `TimestampAsText`; the ISO week from the Thursday of its week). Differentially verified against PostgreSQL 16 on year-boundary dates (ISO weeks 53 and 1). Other fields stay refused by leaf.

## 0.25.1 - 2026-10-22

- **`::regclass` is read.** `SqlType::Regclass` (leaf `regclass`) is new; PostgreSQL to PostgreSQL passes it through, and SQLite, MySQL and MariaDB refuse with the leaf `cast.to_regclass` (no catalog to resolve a relation name against). `to_regclass()` stays refused as before.

## 0.25.0 - 2026-10-22

- **Breaking:** `TableRef::Subquery` gained a `lateral: bool` field; code that builds or exhaustively matches it must be updated.
- **`LEFT JOIN LATERAL (subquery)` is read and translated.** `TableRef::Subquery` gained a `lateral: bool` field (source-breaking for code that builds or exhaustively matches it). PostgreSQL and MySQL >= 8.0.14 keep `LATERAL` (differentially verified against PostgreSQL 16 / MySQL 8.4); MariaDB and SQLite have no equivalent and refuse with the leaf `lateral`. `CROSS JOIN LATERAL <table function>` is still not read.

## 0.24.19 - 2026-10-20

- **Docs only.** `docs/MACROS.md` records why dialect-gating cargo features for `unisql-macros` are not planned (measured: 1.7 to 2.8 s per dialect crate of a 96 s clean build) and that slices 1 to 5 are built; ROADMAP row 3 follows.

## 0.24.18 - 2026-10-20

- **`sql!` syntax errors show the line and a caret** when the parser gave a position (`Line: n, Column: m`, read out of the message; `ReadError::Syntax` is unchanged). An error without a position is shown as before.
- **`sql!` keeps a parsed schema** for the life of the compiler process (keyed by kind, dialect and text; only successful reads), so a crate naming one schema in many calls reads it once. Measured costs are in `docs/MACROS.md` section 6, with `tests/cost.rs` as loose regression guards.
- **crates.io:** 0.24.15 and 0.24.16 were not published; their changes are in 0.24.17 (the next published version after 0.24.14).

## 0.24.17 - 2026-10-20

- **`Column` gains `declared_type`, `nullable` and `has_default`** (all `Option`: `None` is unknown, never "no"), filled by `schema_from_ddl` from the column definition, `ALTER COLUMN SET/DROP NOT NULL`, `SET/DROP DEFAULT`, `SET DATA TYPE` (which keeps the other two) and a table-level `PRIMARY KEY`. A serial, identity or `AUTO_INCREMENT` column has a default; `NOT NULL`, a primary key and serial columns are not nullable. `Column` equality now also compares them; the scope resolution that decides whether two same-named columns agree still compares class and collation only.
- **`Schema::tables()`** enumerates tables as `TableView` (name, columns, unique keys, `keys_unreliable`, `columns_complete`).
- **Feature `serde`** (`unisql`, `unisql-core`): serde on `Schema`/`Column`/`ColumnClass` and `Snapshot` / `SnapshotError` / `SNAPSHOT_FORMAT` (a JSON file with a `format` integer; a newer format is refused before its shape is read; unknown fields are ignored and missing optional ones read as unknown; a `meta` map is kept for the producer). `unisql-macro-core` and `unisql-macros` enable it; `sql!(schema = "x.json")` reads a snapshot.

## 0.24.16 - 2026-10-20

- **`sql!` reads a schema** (slice 4 of `docs/MACROS.md`): `schema = "path.sql"` (DDL in the source dialect, relative to the crate's manifest directory, included so cargo rebuilds when it changes) or `schema_ddl = "CREATE TABLE .."`. It feeds the translator (a comparison on a non-text column no longer asks for `text_collation`) and `SchemaChecker`: an `INSERT` row whose width differs from its column list, a column listed twice, and, where the schema states a table's columns in full, an unknown column in `INSERT`/`UPDATE .. SET` or a row of the wrong width for an `INSERT` with no column list. It reports only what is certain; every problem is reported, not the first.
- **`unisql-macro-core`**: the `Checker` trait, `CheckContext` (non-exhaustive), `expand_with(&Input, &[&dyn Checker])`, `Kind::Catalog`, `Input::schema`. Checkers run after the syntax check and before translation. **`unisql`**: `read_statement(sql, source)` returns the IR without translating.

## 0.24.15 - 2026-10-19

- **`sql!` takes arguments** (slice 3 of `docs/MACROS.md`): `sql!(.., "SELECT .. $1 .. $2", a, b)` checks at build time that the count equals the statement's parameters (the error points at the arguments) and expands to `(Query, (a, b))`. Nothing is bound. With no arguments the expansion is the bare `Query`, as before. `allow`, `requires`, `forbids` and `Query::constructs` were already in 0.24.14.

## 0.24.14 - 2026-10-19

- **New crate `unisql-macros`** (slice 2 of `docs/MACROS.md`): `sql!(source = .., targets = [..], allow = [..], requires = [..], forbids = [..], crate = path, "SQL")` expands to a `const` `unisql_embed::Query`. Syntax errors, skipped placeholders, refusals, a missing allowance, an unknown dialect or allowance name, and failed `requires`/`forbids` are compile errors pointing at the argument that is wrong (the statement literal for the SQL itself). 14 `trybuild` cases, every fail case with a passing twin. `unisql-macro-core` re-exports `ParamBinding`.
- `docs/MACROS.md` section 4: the `unisql-macros` cargo features that gate dialects are deferred until `unisql` itself gates them (it always compiles all four today).

## 0.24.13 - 2026-10-18

- **New crates, not yet published:** `unisql-macro-core` (`expand`: checks a call's source, targets, allowances, placeholders, refusals and `requires`/`forbids`, and translates for each named target) and `unisql-embed` (`Query`, `Rendered`, `Target`, `Binding`: `const`-constructible per-target SQL as `&'static` data, `no_std`). Slice 1 of `docs/MACROS.md`; no change to the existing crates.

## 0.24.12 - 2026-10-18

- **Docs only.** `docs/MACROS.md`: design for compile-time translation and database-free checks as macro crates, and ROADMAP row 3 for them. No code change.

## 0.24.11 - 2026-10-17

- **Fix:** `ON CONFLICT .. DO UPDATE SET .. WHERE` read its `t.col` and `EXCLUDED.col` without the schema, so a
  comparison on numeric columns named `text_comparison` (and demanded `text_collation`) on a target that has one. The upsert's
  table is now in scope under its own name and as `excluded`.

## 0.24.10 - 2026-10-17

- **CI only.** Every cargo step in the workflow now passes `--locked`, so CI builds the committed `Cargo.lock`.

## 0.24.9 - 2026-10-16

- **Docs only.** `Allowance::UpsertWhereAsIf` and the 0.24.7 note now say that sqlx-mysql 0.9.0 always sends
  `CLIENT_FOUND_ROWS` (no option), so sqlx callers see 1 inserted, 2 updated, 1 kept or unchanged (measured on MySQL 8.4)
  and cannot tell an insert from a kept row by the count.

## 0.24.8 - 2026-10-16

- **`FROM generate_series(a, b[, step]) AS g`** now translates to SQLite and MySQL/MariaDB as a derived table
  over `WITH RECURSIVE`, for two or three integer literal arguments (step not 0) and an alias; the series
  column carries the alias. Ascending, descending and empty ranges agree with PostgreSQL in the differential
  suites (PostgreSQL 16, SQLite, MySQL 8.4, MariaDB 11.4). Non-literal arguments are refused
  (`table_function.generate_series.args`), as is a missing alias (`table_function.generate_series.alias`);
  other table functions are still refused by leaf. `ORDER BY` on the series column on SQLite needs the
  `text_collation` allowance, as the column's type is not known. New `Target::series_as_recursive_cte`
  (default `false`).

## 0.24.7 - 2026-10-16

- **`ON CONFLICT .. DO UPDATE .. WHERE`** now translates to MySQL/MariaDB as `col = IF(cond, new, col)` in every
  `SET` expression, under the new `Allowance::UpsertWhereAsIf`. The column the condition reads is assigned last
  (MySQL assigns left to right); a condition reading two or more set columns is refused
  (`upsert_where.two_columns`). Rows affected, measured on MySQL 8.4 and MariaDB 11.4: 1 inserted, 2 updated,
  0 kept by the condition and 0 for an update that changes nothing; do not set `CLIENT_FOUND_ROWS`, which
  counts a kept row as 1. A parameter in the condition is bound once per set column. New `Target::upsert_where_as_if`
  (default `false`).

## 0.24.6 - 2026-10-16

- **SQLite `INSERT OR IGNORE`** is read as `INSERT IGNORE` (`ON CONFLICT DO NOTHING` with no target), with
  the same `ignore_covers_only_unique_conflicts` allowance; other `OR` conflict clauses are still refused.
  No AST change.

## 0.24.5 - 2026-10-16

- **SQLite `INSERT OR REPLACE`** is read as `REPLACE INTO` (the IR's `replace_into`), so it now translates
  wherever `REPLACE INTO` does; other `OR` conflict clauses are still refused. No AST change.

## 0.24.4 - 2026-10-15

- **`EXTRACT(EPOCH FROM clock_timestamp())`** (PostgreSQL source) is mapped to the database server clock with
  its fraction: `UNIX_TIMESTAMP(SYSDATE(3))` on MySQL/MariaDB (exact: read at the call), and
  `unixepoch('now', 'subsec')` on SQLite behind the new allowance `clock_is_statement_start` (SQLite
  reads the clock once per statement and needs 3.42+). Multiply by 1000 and cast for epoch milliseconds.
  Differential cases against the real engines. No AST change.

## 0.24.3 - 2026-10-15

- **`ORDER BY` a column the schema says is not text** (bytes, numbers, dates) no longer needs the
  `text_collation` allowance: its ordering involves no collation, as for ordered comparison. A text
  column still needs it. No AST change.

## 0.24.2 - 2026-10-15

- **`ADDDATE(x, n)` / `SUBDATE(x, n)` with a plain number** (MySQL/MariaDB source) are read as
  `x +/- INTERVAL n DAY`, so the existing interval arithmetic applies on every target. Differential cases
  against the real engines. No AST change.

## 0.24.1 - 2026-10-15

- MySQL/MariaDB `CAST(x AS CHAR(n))` is read as a varchar(n) cast (truncates, no padding). Written as
  `CHAR(n)` on MySQL/MariaDB, `VARCHAR(n)` on PostgreSQL and `SUBSTR(CAST(x AS TEXT), 1, n)` on SQLite.
  `CONVERT(.. USING charset)` stays refused (`convert_form`). Differential cases against the real engines.

## 0.24.0 - 2026-10-15

- **Breaking (`Target` trait):** `update_from_tier` is replaced by `dml_sources` (`DmlSources::FromClause`,
  `Join`, `UpdateFromOnly`). **Multi-table `UPDATE .. JOIN` and `DELETE t FROM .. JOIN`** (MySQL/MariaDB
  source) are read into `Update.from` / `Delete.using` with the inner-join conditions moved into `WHERE`;
  `SET u.col` qualified with the target is read as `col`. Written as `FROM`/`USING` on PostgreSQL, `UPDATE ..
  FROM` on SQLite (`DELETE .. USING` refused there, leaf `delete_using`) and a comma join on MySQL/MariaDB,
  so PostgreSQL `UPDATE .. FROM` and `DELETE .. USING` now also translate to MySQL/MariaDB. Outer joins,
  `USING`/`NATURAL` joins, `SET` on a joined table and `DELETE a, b FROM ..` are refused; `ORDER BY`/`LIMIT`
  with joined tables is refused on every target. MySQL's `DELETE FROM t USING <joined source>` form is
  refused (`delete_using_form`) instead of read as PostgreSQL's. Differential cases against the real engines.

## 0.23.0 - 2026-10-15

- **Breaking (AST):** `UPDATE|DELETE .. ORDER BY .. LIMIT n` (MySQL/MariaDB source) is read into the IR
  (`Update` and `Delete` gain `order_by` and `limit`). Written natively on MySQL/MariaDB and as
  `WHERE ctid|rowid IN (SELECT ctid|rowid FROM t WHERE .. ORDER BY .. LIMIT n)` on PostgreSQL and
  SQLite (leaves `update_limit`, `delete_limit`; refused when the statement has FROM/USING tables).
  Differential cases against the real engines.

## 0.22.3 - 2026-10-15

- **`DATE_ADD` / `ADDDATE` / `DATE_SUB` / `SUBDATE` with an `INTERVAL`** (MySQL/MariaDB source) are read as
  `x +/- INTERVAL n unit`, so the existing interval arithmetic applies on every target (months and years
  stay refused, `fn:make_interval.month_year`). The day-count form `ADDDATE(x, 7)` is not read.
  Differential cases against the real engines. No AST change.

## 0.22.2

- Unary `~x` is refused with a named `bitwise_not` reading error instead of the generic `unary_operator`
  one: its width and signedness differ per engine (MySQL yields a 64-bit unsigned number).

## 0.22.1

- Bit literals (`b'101'`, `B'101'`) are refused with an explicit `bit_literal` reading error instead of the
  generic `literal` one: PostgreSQL reads them as `bit(n)`, MySQL as a byte string, SQLite not at all.

## 0.22.0

- **Breaking:** hex string literals are read into the IR (`Literal::Bytes`, an AST break: `Literal` gains a
  variant). `X'41'` and MySQL `0x41` on MySQL/MariaDB, `X'41'` on SQLite. Written as `X'41'` on
  MySQL/MariaDB and SQLite and as `'A'::bytea` on PostgreSQL. PostgreSQL's `X'..'` is a bit string and
  is refused (`hex_literal`), as is an odd-length or non-hex body. Arithmetic or bitwise use
  (`0x41 + 1`, which MySQL reads as the number 65) is refused (`hex_literal_numeric`). Bit literals
  (`b'101'`) remain unread.

## 0.21.0 - 2026-10-15

- **Breaking:** MySQL/MariaDB `GROUP BY a, b WITH ROLLUP` is read into the IR (`Select.group_by_rollup`, an AST
  break: `Select` gains a field). Written as `ROLLUP (a, b)` on PostgreSQL and `WITH ROLLUP` on
  MySQL/MariaDB; refused on SQLite (`group_by.rollup`). `CUBE`, `TOTALS` and `GROUPING SETS` stay
  refused (`group_by_modifier`).

## 0.20.5 - 2026-10-15

- **`CEIL`/`CEILING`/`FLOOR` and `MOD(a, b)`** (differential cases against the real engines):
  `CEIL(x)`, `CEILING(x)` and `FLOOR(x)` are one IR call each (`ceil`, `floor`), written natively on
  PostgreSQL and MySQL/MariaDB and refused on SQLite (`fn:ceil`, `fn:floor`: its math functions are a
  compile-time option, so a mapping cannot be verified). `MOD(a, b)` is read as `a % b`. The unit and
  scale forms (`CEIL(x TO DAY)`) are refused by the reader (`ceil_form`). No AST change.

## 0.20.4 - 2026-10-15

- **String search with verified mappings** (differential cases against the real engines): MySQL/MariaDB
  `LOCATE(sub, str)` and `INSTR(str, sub)`, standard `POSITION(sub IN str)` and PostgreSQL
  `strpos(str, sub)` are read as one IR call, `strpos`, and written as `strpos` (PostgreSQL),
  `instr` (SQLite) and `INSTR` (MySQL/MariaDB). From a MySQL/MariaDB source the translation is
  conditional on `text_collation` (their comparison follows the column collation). The
  three-argument `LOCATE` (start position) is refused (`fn:locate`). SQLite `instr` now translates
  (it was refused). No AST change.

## 0.20.3 - 2026-10-15

- Docs only: `docs/DESIGN.md` is restated as a plain design rationale (decisions with their reasons;
  the working-arrangement text and quoted instructions are removed), and `docs/MEASUREMENT.md`,
  `docs/LICENCES.md`, `docs/PUBLISHING.md` and a few file comments no longer refer to internal
  working arrangements or local paths. The two private corpora are named `large-app` and `small-app`.
  No code change.

## 0.20.2 - 2026-10-15

- Docs only: wording in this changelog and the ROADMAP no longer refers to the project's internal
  working arrangements. No code change.

## 0.20.1 - 2026-10-15

- MySQL/MariaDB `CONVERT(expr, type)` is read as `CAST(expr AS type)` (so it translates wherever the
  cast does, under `lenient_cast`). `CONVERT(expr USING charset)`, `TRY_CONVERT` and the MSSQL
  target-first form are refused by the reader (`convert_form`). No AST change.

## 0.20.0 - 2026-10-14

- **Breaking (AST):** new `Expr::Regexp { expr, pattern, negated }` for MySQL/MariaDB `REGEXP` and
  `RLIKE` and PostgreSQL `~` / `!~`. Written as `~` / `!~` on PostgreSQL and `REGEXP` / `NOT REGEXP` on
  MySQL/MariaDB, under the new `Allowance::RegexpFlavor` (the engines' regular-expression syntax and
  case sensitivity differ beyond the common subset). SQLite is refused (`regexp`): it has no built-in
  matcher. The case-insensitive `~*` / `!~*` are still not read. Code that matches `Expr` or
  `Allowance` exhaustively must add the arm. `LIKE BINARY` is unchanged (sqlparser reads it as a cast).
- **Breaking (AST):** new `BinaryOp` variants `BitAnd`, `BitOr`, `BitXor`, `ShiftLeft`, `ShiftRight`
  and `Xor`, read from MySQL/MariaDB (`& | ^ << >> XOR`) and PostgreSQL (`& | # << >>`). Bitwise
  operators are written natively and gated by the new `Allowance::BitwiseNonNegativeIntegers` (MySQL
  computes on unsigned 64-bit integers); XOR becomes `<>` on PostgreSQL and SQLite under the new
  `Allowance::XorOperandsBoolean`; bitwise XOR is refused on SQLite (`bitwise.xor`). Not read yet:
  unary `~` (unsigned on MySQL), and `<<`/`>>` from a SQLite source (sqlparser rejects them there).
  `Allowance` is exhaustive, so a `match` on it needs the two arms.

## 0.19.0 - 2026-10-14

- **Breaking (AST):** new `Expr::IsBool { expr, value, negated }` for `expr IS [NOT] TRUE` and
  `IS [NOT] FALSE` (never NULL; `NULL IS TRUE` is false, `NULL IS NOT TRUE` is true). The reader
  produces it from any source dialect and the printer writes it unchanged for PostgreSQL, SQLite and
  MySQL/MariaDB. Differential cases against the real engines compare the NULL arm. Code that matches
  `Expr` exhaustively must add the arm.

## 0.18.3 - 2026-10-14

- Docs only: ROADMAP gains the integration proposal for auth-framework's storage SQL (measured at
  auth-framework `86de4cdf`); no code change.

## 0.18.2 - 2026-10-14

- **MySQL/MariaDB string functions with verified mappings** (differential cases against the real
  engines): `CONCAT_WS` (PostgreSQL; SQLite >= 3.44), `LEFT`/`RIGHT` for a literal count >= 0
  (PostgreSQL native; SQLite `substr`), `REPEAT` for a literal count >= 0 (PostgreSQL native; SQLite
  `replace(hex(zeroblob(n)), '00', s)`), and on PostgreSQL `LPAD`/`RPAD` (literal length >= 0 and
  literal non-empty pad) and `REVERSE`. A negative or non-literal count is refused by leaf
  (`fn:left.length`, `fn:lpad.args`, ...) because the engines differ there. A MySQL/MariaDB source
  passes these through to MySQL/MariaDB unchanged. Statements that were refused now translate; no
  AST change.

## 0.18.1 - 2026-10-14

- Repository hygiene, no code change: `.github/CODEOWNERS` (`* @ciresnave`).

## 0.18.0 - 2026-10-14

- **Breaking (IR, `unisql_core::ast`, re-exported as `unisql::ast`):** `TableRef::Join` gains `using: Vec<String>` and
  `natural: bool`; `Insert` gains `replace: bool`. A consumer that constructs or exhaustively
  destructures these nodes must add the fields.
- **`JOIN .. USING (cols)` and `NATURAL [LEFT|RIGHT|FULL] JOIN` are read and written** by all four
  dialects (leaves `join.using`, `join.natural`, Exact), tested against real engines (inner, left,
  natural, natural left).
- **`REPLACE INTO`** (MySQL/MariaDB) is read (leaf `replace_into`): native `REPLACE INTO` on
  MySQL/MariaDB, `INSERT OR REPLACE INTO` on SQLite (tested), and on PostgreSQL an upsert
  `ON CONFLICT (key) DO UPDATE SET <every other column> = EXCLUDED.<column>` when the schema states
  the table's complete column list and exactly one unique key; allowance `replace_is_upsert` (the old
  row is updated, not deleted: DELETE triggers and `ON DELETE` actions do not fire). Otherwise
  refused (`replace_into`). New: `Schema::mark_columns_complete`, `Schema::complete_columns`
  (`schema_from_ddl` marks every `CREATE TABLE` it reads); `Target::replace_keyword`;
  `Allowance::ReplaceIsUpsert`.

## 0.17.5 - 2026-10-14

- **`INSERT INTO t SET a = 1, b = 2` (MySQL/MariaDB) is read** as `INSERT INTO t (a, b) VALUES (1, 2)`,
  a pure reader desugar (no IR change), tested against real MySQL, MariaDB, PostgreSQL and SQLite.
  `INSERT .. SET` combined with a column list, or with a query, is still refused (`insert_option`).

## 0.17.4 - 2026-10-14

- **MySQL/MariaDB date and time functions with verified mappings** (differential cases against real
  MySQL 8.4, MariaDB 11.4, PostgreSQL and SQLite): `UTC_TIMESTAMP()` and `UTC_TIMESTAMP(6)`
  (PostgreSQL `NOW() AT TIME ZONE 'UTC'`, exact; SQLite `datetime('now')` / `strftime('%f')`,
  conditional on `timestamp_as_text`, and for precision 6 on the new allowance
  `timestamp_millisecond_resolution`; other precisions refused, leaf `fn:utc_timestamp.precision`),
  `UTC_DATE()` (`date_as_text` on SQLite), `CURDATE()` (read as `CURRENT_DATE`), `DATEDIFF(a, b)`
  (PostgreSQL date subtraction under `lenient_cast`; SQLite `julianday` under `timestamp_as_text`) and
  `DATE(x)`. A MySQL/MariaDB source writes its own `UTC_TIMESTAMP`/`UTC_DATE`/`DATEDIFF`/`DATE` back
  unchanged. New leaves `fn:utc_timestamp`, `fn:utc_timestamp.precision`, `fn:utc_date`,
  `fn:datediff`, `fn:date`. Statements that were refused are now translated; no AST change.
- `Allowance` gains `TimestampMillisecondResolution` (the enum is `#[non_exhaustive]`).
- MySQL-source corpus: both auth-framework `UTC_TIMESTAMP(6)` statements translate.

## 0.17.3 - 2026-10-14

- **`Schema` carries unique keys; `ON DUPLICATE KEY UPDATE` translates to PostgreSQL and SQLite.**
  Additive API: `Schema::with_unique_key`, `unique_keys`, `add_unique_key`, `mark_keys_unreliable`,
  `mark_all_keys_unreliable`; `schema_from_ddl` reads `PRIMARY KEY` (column and table level),
  `UNIQUE`, `CREATE UNIQUE INDEX` and `ALTER TABLE .. ADD CONSTRAINT`; a partial or expression index,
  a dropped constraint or any `DROP INDEX` makes the list unreliable (nothing is derived from it).
  A MySQL upsert with no stated key is written with the table's only stated key as the conflict
  target (new leaf `upsert.key_from_schema`, Exact, sound only if the stated keys are all the table
  has); with several keys, none or an unreliable list it is refused as before
  (`upsert.no_conflict_key`, the message now says the schema must state exactly one). `Target` gains
  `needs_conflict_key` (default true; false for MySQL/MariaDB). No existing public item changes: the new `Ctx` field is crate-private.
- **Fixed:** an unqualified column in a MySQL-written upsert SET list (`n = n + 1`) is now written
  `"table"."n"`: PostgreSQL finds it ambiguous with `EXCLUDED`. A subquery's own columns are not
  touched. Previously unreachable (the statement was refused), found by the new differential cases.
- Acceptance: auth-framework's 3 `ON DUPLICATE KEY UPDATE` statements (MySQL-source corpus) now
  translate to PostgreSQL and SQLite and are accepted by those engines; 3 new differential cases
  (duplicate key, new key, two keys refused) against real MySQL/MariaDB, PostgreSQL and SQLite.

## 0.17.2 - 2026-10-14

- **MySQL-source corpus and harness** (`crates/unisql-differential/corpus/mysql/`, test
  `mysql_corpus`): 129 statements validated on real MySQL 8.4 and MariaDB 11.4, translated to
  PostgreSQL, SQLite and MariaDB, the output prepared on the target engines; verdicts in
  `expected.tsv`; the gap list is in ROADMAP.md. `corpus` binary: `UNISQL_SHOW_SQL=1` prints the
  translated text, and it grants `Allowance::ALL`.
- **Fixed, found by the corpus** (statements that were mistranslated now read or refuse):
  - `schema_from_ddl` panicked on a quoted identifier in DDL (`` `key` ``, `"Key"`).
  - `UPDATE LOW_PRIORITY|IGNORE t ..` was written with a table named `low_priority`/`ignore`:
    now refused (`update_option`).
  - `DEFAULT` as a value (`VALUES (?, DEFAULT)`) was written as the identifier `"default"`: now
    refused by the reader (`default_value`), in every source dialect.
  - SQLite output `group_concat(DISTINCT x, ',')` is invalid in SQLite: now `group_concat(DISTINCT x)`
    for the default separator, refused (`fn:string_agg`) for another.
  No AST or API change; behaviour changes only where the previous output was wrong.

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

Gap sweep over the statements of auth-framework (`origin/main` 1b31e902) and lightbulb
(`origin/main` c5b7fd77), read from those commits, never a working tree.

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

Verify and enable one mapping per refused leaf, largest first. Each mapping below was run
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
  PersistAnt for deriving a program's needs from its statement catalogue.

## 0.0.2

- A local working-notes file is no longer tracked and is ignored (hygiene; no code change).

## 0.0.1

- First tracked state of UniSQL: IR, PostgreSQL/SQLite/MySQL/MariaDB readers and writers, parameter
  codec for array parameters, differential suites against real engines, matrix files, first CI.
