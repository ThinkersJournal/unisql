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

1. **Date and time functions** (`date_trunc`, `to_char`, `extract`, `age`, `date_part`). The largest
   remaining corpus residual. Start from what the three engines agree on, and refuse the rest by
   leaf (`fn:date_trunc.week` etc.) rather than the whole function.
2. **MySQL reader gaps:** `||` (MySQL means OR; the parser gives it concatenation precedence),
   `SUBSTRING(s FROM n)` (start-position semantics), `INSERT IGNORE`, `<=>`. Each is a reader rule in
   `unisql-sqlparser-adapter` or a parser-level workaround; none needs an IR change except `<=>`.
3. **Schema input.** Whether `age > $1` compares numbers or text is syntactic today, so it records
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
