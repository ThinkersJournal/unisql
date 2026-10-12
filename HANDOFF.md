# unisql lane HANDOFF (current state only; rewrite, never append)

Written 2026-10-12 after milestones A-E and the first push. Lane `unisql`, cwd `C:\Projects\unisql`.
**Remote: `origin` = https://github.com/ThinkersJournal/unisql (PRIVATE, created by the PM; board 182:
CireSnave "ThinkersJournal/unisql using unisql on Crates.io").** `main` = 7340fd0 (pushed, history intact).
First CI-workflow PR = #1 (branch `ci/first-workflow`); the PM sets branch protection after its checks exist.
Rules: Rust only; crates.io deps only; `MIT OR Apache-2.0`; one version number (workspace 0.0.1); no Opus;
every subagent `model: sonnet|haiku`; **do NOT make the repo public, publish, or reserve the crate name**
(`publish = false` on every crate); never touch `gh auth`; remove engine containers when you stop.

## State
- Crates: `unisql` (facade `translate`), `unisql-core` (IR, tiers, report, printer, `Target` hooks, rewrites,
  `ParamBinding`), `unisql-sqlparser-adapter` (sqlparser AST -> IR, `Rules` per dialect, replaceable),
  `unisql-dialect-{postgres,sqlite,mysql}` (all four dialects read AND write), `unisql-differential`.
- Differential vs REAL engines, 0 FAIL (docs/DIFFERENTIAL-*.txt): pg->sqlite 120, sqlite->pg 53, pg->mysql 83,
  pg->mariadb 85, mysql->pg 41, mysql->sqlite 40, mariadb->pg 41, + core unit tests 2. Engines: PostgreSQL 16.15
  (`--locale=C`), SQLite 3.53.2 bundled, MySQL 8.4.11 / MariaDB 11.4.13 with `utf8mb4_bin`; commands + digests in
  README. Containers REMOVED at stop: recreate per README (suites fail, never skip, if unreachable).
- CI (PR #1, run 38292311341, all 6 jobs green on the first run): `Build, test, clippy, fmt` (ubuntu +
  windows), `MSRV` (rustc 1.94.1), `cargo deny`, `Every source file declares the licence`, `Differential suites
  ...` (engines pinned by digest; ends with `git diff --exit-code` on matrix files and run logs). Names and
  action pins mirror PersistAnt's workflow (cc934e10).
- Matrix files (PersistAnt row schema, acked by their lane): `crates/unisql-dialect-*/matrix-<backend>-from-
  <source>.tsv`; `how=conditional` carries `allowance=` first in note; EVERY refusal is subject
  `<construct>.refused` (= `Untranslatable::subject()`).
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
