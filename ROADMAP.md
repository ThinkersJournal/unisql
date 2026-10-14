# UniSQL roadmap

Written 2026-10-14 by the unisql lane at origin/main 0844289 (0.2.0). It records what is *not* done,
in the order the lane intends to take it, with the reason and how each item would be verified. It is
not a promise of dates, and the PM may reorder it. Direction comes from `docs/DESIGN.md` (sections
0.1, 2.1, 2.2, 5 carry the user's words verbatim): comprehensive dialect coverage rather than
consumer-driven, dialects modular, the parser replaceable, SQLGlot the primary reference.

Every item ships with a differential test against the real engines (digest-pinned in
`.github/workflows/ci.yml`), a matrix row (leaf id plus contract line in `docs/LEAF-CONTRACTS.tsv`),
and a CHANGELOG entry; a form with no verified mapping is refused, never guessed.

## Next

1. **`LIKE` / `ILIKE` under a collation (MySQL, MariaDB).** Today `like` is conditional on
   `like_case_sensitivity_is_connection_setting`, which is true of SQLite but not of MySQL, where `LIKE`
   follows the column collation. Split the cause: on MySQL/MariaDB read it from `config` (as
   `text_comparison` already is) and add a known-bad control under the default collation that must
   diverge. *Verified by:* four probes (`LIKE 'b'`, `LIKE 'b%'`, `ILIKE`, a pattern with `_`) run under
   `utf8mb4_bin` (agree) and the default collation (must diverge).
2. **Row locks: `FOR UPDATE OF`, `NOWAIT`, `SKIP LOCKED`.** The IR has `row_lock` only. Residual in the
   Humboldt corpus (`docs/CORPUS-*-D.txt`), and PersistAnt names `rel.row_lock(skip_locked)`. PostgreSQL
   and MySQL 8 have all three, MariaDB has `NOWAIT` and `SKIP LOCKED` (versions to be read from its documentation and tested on the pinned 11.4, not assumed), SQLite none (refused, as now).
   *Verified by:* two-connection tests (a locked row is skipped / raises) on the engines.
3. **Date and time functions** (`date_trunc`, `to_char`, `extract`, `age`, `date_part`). The largest
   remaining corpus residual. Start from what the three engines agree on, and refuse the rest by
   leaf (`fn:date_trunc.week` etc.) rather than the whole function.
4. **MySQL reader gaps:** `||` (MySQL means OR; the parser gives it concatenation precedence),
   `SUBSTRING(s FROM n)` (start-position semantics), `INSERT IGNORE`, `<=>`. Each is a reader rule in
   `unisql-sqlparser-adapter` or a parser-level workaround; none needs an IR change except `<=>`.
5. **Schema input.** Whether `age > $1` compares numbers or text is syntactic today, so it records
   `text_comparison`. An optional column-type map would make that exact and shrink the number of
   statements that need an allowance. *Open design question:* where the map comes from (PersistAnt's
   catalog read, or the caller).

## Later

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
- **Publishing.** `publish = false` on every crate; names are held as 0.0.0 placeholders by the PM.
  The first release and the public flip are CireSnave's and the PM's call.
