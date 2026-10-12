# unisql lane HANDOFF (current state only; rewrite, never append)

Written 2026-10-12. Lane `unisql`, cwd `C:\Projects\unisql`. **Local git only: `main`, no remote, nothing
published, no name reserved; every crate `publish = false`.** Rules: Rust only; crates.io deps only;
`MIT OR Apache-2.0`; no Opus; every subagent `model: sonnet|haiku`; never push/publish/create a repo (the PM
creates repos; D7 = board 182 must be answered first); never touch `gh auth`.

## State (last local commit: see `git log`)
- Workspace: `unisql` (facade `translate`), `unisql-core` (IR, tiers, report, printer, `Target` hooks),
  `unisql-dialect-postgres` (reader over sqlparser 0.63.0 + target), `unisql-dialect-sqlite` (target),
  `unisql-differential` (harness + `tests/postgres_to_sqlite.rs`).
- Differential: 74/74 pass vs real PostgreSQL 16.15 (`--locale=C`) and SQLite 3.53.2; output in
  `docs/DIFFERENTIAL-postgres-sqlite.txt`. Needs container `unisql-pg` on 127.0.0.1:55432
  (`docker start unisql-pg`; recreate per README). It fails, never skips, if unreachable.
- Licences read (docs/LICENCES.md): SQLGlot MIT (derivable, keep notice), jOOQ Apache-2.0 + commercial.
  Logo = Openclipart "Unicycle" by GDJ, public domain, SVG byte-verified.
- User direction (verbatim in DESIGN 0.1/2.1/2.2/5): comprehensive modular dialects, sqlparser replaceable,
  SQLGlot primary reference, API = text+source+target in, Result<Translation> out, PersistAnt changeable but
  PM decides disagreements.

## Open / next
- Next slice candidates: SQLite reader (so SQLite->Postgres); DISTINCT ON emulation; NOWAIT/SKIP LOCKED in IR;
  window functions; PRAGMA-off control case for LIKE; map the 44 unmapped Humboldt functions with tests;
  matrix file in the agreed row schema (DESIGN 5.4); MySQL dialect to test the trait boundary (DESIGN 2.1).
- Any code derived from SQLGlot: trace file+SHA, keep MIT notice (LICENCES.md).
- Rule table in `measure/` and tiers in DESIGN 3.1 share an author; the differential suite is the check.
