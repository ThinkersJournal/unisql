# unisql lane HANDOFF (current state only; rewrite, never append)

Written 2026-10-12 after milestones A, B, C. Lane `unisql`, cwd `C:\Projects\unisql`.
**Local git only: `main`, no remote, nothing published, no name reserved; every crate `publish = false`.**
Rules: Rust only; crates.io deps only; `MIT OR Apache-2.0`; no Opus; every subagent `model: sonnet|haiku`;
never push/publish/create a repo (the PM creates repos; D7 = board 182 must be answered first); never touch
`gh auth`; remove the engine containers when you stop.

## State (last local commit: see `git log`)
- Crates: `unisql` (facade `translate`), `unisql-core` (IR, tiers, report, printer, `Target` hooks, rewrites,
  `ParamBinding`), `unisql-sqlparser-adapter` (sqlparser AST -> IR, `Rules` per dialect, replaceable),
  `unisql-dialect-{postgres,sqlite,mysql}` (all four dialects read AND write), `unisql-differential`.
- Differential vs REAL engines, 0 FAIL (docs/DIFFERENTIAL-*.txt): pg->sqlite 120, sqlite->pg 53, pg->mysql 83,
  pg->mariadb 85, mysql->pg 41, mysql->sqlite 40, mariadb->pg 41, + core unit tests 2. Engines: PostgreSQL 16.15
  (`--locale=C`), SQLite 3.53.2 bundled, MySQL 8.4.11 / MariaDB 11.4.13 with `utf8mb4_bin`; commands + digests in
  README. Containers REMOVED at stop: recreate per README (suites fail, never skip, if unreachable).
- Matrix files (PersistAnt row schema, agreed with their lane): `crates/unisql-dialect-*/matrix-<backend>-from-
  <source>.tsv`; `how=conditional` carries `allowance=` first in note; EVERY refusal is subject `<construct>.refused`
  (= `Untranslatable::subject()`), so a gate keys on the same string the matrix holds.
- Param codec: `Translation::param_bindings` (Scalar | NativeArray | JsonArrayText) next to `param_order`;
  `= ANY($n)` works pg/sqlite/mysql/mariadb; a target with no codec refuses `array_param`.
- Allowances: 15 named (see `report.rs`); no blanket mode.
- Corpus (docs/CORPUS-*-D.txt, real translator, DML, humboldt src n=339 @b7cd95e7): ->sqlite 279 translate
  (82.3%), ->mysql 261 (77.0%), ->mariadb 278 (82.0%); text produced, NOT executed on those schemas.
- Licences (docs/LICENCES.md): SQLGlot MIT, jOOQ Apache-2.0+commercial; nothing derived from either yet.
- User direction (verbatim in DESIGN 0.1/2.1/2.2/5): comprehensive modular dialects, sqlparser replaceable,
  SQLGlot primary reference, API = text+source+target -> Result<Translation>, PersistAnt changeable but PM
  decides disagreements.

## Known limits / open
- MySQL reader refuses `||` (parser gives it concatenation precedence, MySQL means OR), `SUBSTRING`
  (start-position semantics), `INSERT IGNORE`, `<=>`; ODKU reads but PG/SQLite refuse it (no conflict key).
- Residual not_in_ir (Humboldt src): PG catalog types (regclass, correct), FOR UPDATE OF/NOWAIT/SKIP LOCKED,
  function over subquery, `EXTRACT(EPOCH ..)`; `TRIM(LEADING FROM x)` is a sqlparser gap.
- Identifier case: MySQL/SQLite sources fold all identifiers to lower case (assumes a migrated lowercase schema).
- No DDL translation (D6, PersistAnt), no matrix merge reader (PersistAnt's side).
- Next candidates: FOR UPDATE OF/NOWAIT/SKIP LOCKED in the IR; date/time functions (date_trunc, to_char,
  extract); more mappings with tests; derive from SQLGlot only with file+SHA trace and MIT notice.
