# unisql lane HANDOFF (current state only; rewrite, never append)

Written 2026-10-12 after milestones A, B, C. Lane `unisql`, cwd `C:\Projects\unisql`.
**Local git only: `main`, no remote, nothing published, no name reserved; every crate `publish = false`.**
Rules: Rust only; crates.io deps only; `MIT OR Apache-2.0`; no Opus; every subagent `model: sonnet|haiku`;
never push/publish/create a repo (the PM creates repos; D7 = board 182 must be answered first); never touch
`gh auth`; remove the engine containers when you stop.

## State (last local commit: see `git log`)
- Crates: `unisql` (facade `translate`), `unisql-core` (IR, tiers, report, printer, `Target` hooks, rewrites),
  `unisql-sqlparser-adapter` (sqlparser AST -> IR, `Rules` per dialect, replaceable),
  `unisql-dialect-{postgres,sqlite,mysql}` (readers: postgres, sqlite; targets: postgres, sqlite, mysql, mariadb),
  `unisql-differential` (harness, 4 suites, `corpus` bin).
- Differential vs REAL engines (docs/DIFFERENTIAL-*.txt): pg->sqlite 106/106, sqlite->pg 52/52,
  pg->mysql 8.4.11 71/71, pg->mariadb 11.4.13 72/72. Engines: PostgreSQL 16.15 (`--locale=C`), SQLite 3.53.2
  bundled, MySQL/MariaDB with `utf8mb4_bin`; run commands + image digests in README.md. Containers REMOVED at stop:
  recreate per README before `cargo test -p unisql-differential` (it fails, never skips, if unreachable).
- Matrix files (PersistAnt row schema) per module: `crates/unisql-dialect-*/matrix*.tsv`, written by the suites.
  Conditional is recorded as `how=emulated` + note (PersistAnt said Conditional is a call-site allowance, not a
  cell; confirm the mapping with the PersistAnt lane).
- Corpus, real translator, DML, all allowances granted (docs/CORPUS-*.txt; SHAs auth@4582a64f
  lightbulb@34e5432b humboldt@b7cd95e7; text produced, NOT executed on those schemas). Humboldt src n=339:
  sqlite 169 translated/106 conditional/24 refused/40 not_in_ir; mysql 24/233/42/40; mariadb 35/241/23/40.
- Licences (docs/LICENCES.md): SQLGlot MIT, jOOQ Apache-2.0+commercial; nothing derived from either yet.
  Logo: Openclipart "Unicycle" by GDJ, public domain, SVG byte-verified.
- User direction (verbatim in DESIGN 0.1/2.1/2.2/5): comprehensive modular dialects, sqlparser replaceable,
  SQLGlot primary reference, API = text+source+target -> Result<Translation>, PersistAnt changeable but PM
  decides disagreements.

## Known limits / open
- Residual not_in_ir (Humboldt src): `= ANY($1)` array params (binding-shape change, needs a param-codec
  decision), aggregate ORDER BY (string_agg(.. ORDER BY ..)), PG catalog types (regclass: refusal correct),
  `EXTRACT(EPOCH FROM ..)`, `TRIM(LEADING FROM x)` is a sqlparser gap.
- MariaDB `UPDATE .. RETURNING` is not detected as unsupported (hook has no statement kind).
- No MySQL reader, no DDL translation (D6: DML first), no matrix merge reader (PersistAnt's side).
- Rule table in `measure/` and DESIGN 3.1 share an author; the differential suites are the check.
- Next candidates: `= ANY` via json_each + param codec; aggregate ORDER BY; MySQL reader; more mappings
  (date_trunc, to_char, extract); derive from SQLGlot only with file+SHA trace and MIT notice.
