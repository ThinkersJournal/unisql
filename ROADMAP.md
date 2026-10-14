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
   `multiple_statements` 34 are DDL and scripts, out of scope.) Next: the MySQL reader gaps below (no corpus statement left in DML that the IR cannot read
   except none: `current_user`, `Extract` and `Array` are read and refused by leaf).
   allowance names that MySQL `UUID()` is a v1 UUID.
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
