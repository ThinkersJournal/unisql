# unisql lane HANDOFF (current state only; rewrite, never append)

Written 2026-10-12. Lane `unisql`, cwd `C:\Projects\unisql`. **Local git only: `main`, no remote, nothing
published, no name reserved.** Rules: Rust only; crates.io deps only; `MIT OR Apache-2.0`; no Opus; every
subagent `model: sonnet|haiku`; never publish, create a repo, or touch `gh auth`.

## State
- `docs/DESIGN.md` (direction 0.1, dialect modules 2.1, D5 settled s5.3-5.6, D8/D9), `docs/MEASUREMENT.md`
  (counts, pinned SHAs, oracle finding). `measure/` = throwaway extractor + classifier + report scripts.
  Reproduce: `python3 -I measure/extract.py <repo> <sha> <corpus> > x.jsonl`; `cargo build --release` in
  `measure/classify`; `classify < x.jsonl > x.out.jsonl`; `summarise.py`, `final.py`, `report.py`.
- SHAs measured: auth-framework 4582a64f, lightbulb 34e5432b, Humboldt b7cd95e7 (re-fetch moves them).
- Last local commit on `main`: see `git log`; DESIGN 2.1 edit and this HANDOFF may be uncommitted.

## Task from PM 2026-10-12: [TASK] measurement + D5. DONE, reported
- D5 agreed with PersistAnt lane (plain-string ids, PersistAnt owns vocabulary + row schema, each repo its own
  matrix file, `rel.source_dialect(d)` separate from `rel.dialect`). PersistAnt sends the PM the one-line [FINDING].
- Measurement: [FINDING] to PM. D7 (repo/name/owner) is the PM's board item; do nothing until answered.

## User direction received in-session 2026-10-12 (all recorded in DESIGN 0.1/2.1)
R2 per-statement/project source dialect; sqlparser replaceable; SQLGlot primary source; jOOQ worth a look;
Diesel/sqlx as integration targets; comprehensive dialect coverage (not consumer-driven); dialects modular.
"jQQQ" read as jOOQ.

## Open / next (none unblocked without a PM [TASK])
- SQLGlot and jOOQ licences are UNVERIFIED: read LICENSE in each repo before any derivation (EXPECTATIONS 6.4a).
- Unmapped functions (current_schema, make_interval, md5, current_date ...) need classification.
- Differential layer (Postgres vs SQLite oracle) not built; every tier is a rule-table proposal.
- Build phase needs a PM [TASK] and D7.
