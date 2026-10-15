# UniSQL corpus measurement (design phase, section 7 of DESIGN.md)

Measured 2026-10-12, in the design phase. **No emitter exists; nothing was translated or executed.**
Every count below is "how many statements contain which constructs, by a rule table", not "how many
translate correctly". Tiers are a **proposal** (DESIGN.md section 3.1) until the differential layer runs.

## Refs (all read with `git show <sha>:<path>`, never a working tree)

| Corpus | Repo | `origin/main` SHA | Paths read |
|---|---|---|---|
| auth-framework | `auth-framework` | `4582a64fddbb5a687fbe04381f913e87b29a8b06` | `src/storage/`, `auth-storage-sqlite/`, `src/migrations/` |
| small-app (a Postgres service) | private repository | `34e5432bda04e1726aaa5e1077b60bb2d7740d4d` | whole tree (`.rs`, `migrations/*.sql`) |
| large-app (a Postgres application) | private repository | `b7cd95e72a1269ad2abf0a9a2cf5725d39eb69fb` | whole tree (`backend/src`, `backend/tests`, `backend/migrations`) |

Each repo was `git fetch`ed first; the SHA is `git rev-parse origin/main` after that fetch.

## Method

1. `measure/extract.py` pulls every Rust string literal (normal and raw, comments skipped) whose first
   word is a SQL keyword and has two or more words, plus every `.sql` file, at the pinned SHA.
2. `measure/classify` (throwaway Rust, `sqlparser` 0.63.0, `visitor` feature) parses each text with the
   dialect of its source (PostgreSQL; SQLite for paths containing `sqlite`; MySQL for `mysql`). `{x}`
   `format!` holes are replaced by an identifier **only if the text fails to parse as written**.
   `DO $$ .. $$` blocks (sqlparser cannot parse them) are stripped and counted as refused-procedural.
3. It tags constructs against a rule table in `classify/src/main.rs` and reports the worst tier per
   statement. Functions not in the table are **`unmapped`**, not refused: they are gaps in my table and
   may be emulable. `refused` here means refused *by design* (locks, advisory locks, triggers,
   procedural code, array/JSON operators, session settings).
4. `measure/summarise.py` excludes **51 literals as prose** (parse failed and no SQL-shaped token:
   "Lock poisoned...", "Explain gravity briefly."). Rule: `$n`, `from`, `into`, `set x =`, `table`,
   `trigger`, `database` as whole words. All 51 were read in the output; `.sql` files are never excluded.

**Controls (run in the same invocation as each corpus, `measure/controls.jsonl`):** 12 statements with a
known expected tier, one or more per tier plus one deliberate parse failure: **12 of 12 matched**.
Control types: plain SELECT/DELETE (exact), `DISTINCT ON`, `ON CONFLICT`, `RETURNING` (emulated),
`ILIKE` (conditional), `FOR UPDATE SKIP LOCKED`, `pg_advisory_xact_lock`, `->>`/`@>`, `CREATE TRIGGER`,
`= ANY($1)` (refused), `SELEKT` (unparsed). A control proves the rules fire; it does not prove the
rule table is right about SQLite.

## Results: DML (what D6 puts first)

| Group | n | exact | emulated | conditional | unmapped | refused | unparsed |
|---|---|---|---|---|---|---|---|
| auth-framework `postgres.rs` (Postgres source) | 19 | 8 | 3 | 8 | 0 | 0 | 0 |
| auth-framework `sqlite.rs` (SQLite source, oracle) | 18 | 15 | 3 | 0 | 0 | 0 | 0 |
| auth-framework `auth-storage-sqlite` (SQLite source) | 21 | 18 | 3 | 0 | 0 | 0 | 0 |
| small-app Rust | 8 | 2 | 1 | 4 | 1 | 0 | 0 |
| large-app `backend/src` | 339 | 213 | 28 | 34 | 44 | 20 | 0 |
| large-app `backend/tests` (fixtures) | 970 | 569 | 342 | 37 | 20 | 2 | 0 |

large-app `src`: 275 of 339 (81%) are exact, emulated or conditional; 20 (5.9%) are refused by design;
44 (13%) are unmapped: of those 44 statements, 17 use `current_schema()` and 9 use
`make_interval` (a statement can name several), concentrated in `dbtools.rs` 9, `security.rs` 9,
`transfer.rs` 7, `wipe.rs` 7. These are rule-table gaps, not findings about SQLite. Tests are fixtures
and setup SQL; they are not the portability target and are reported only so the totals are complete.
Construct occurrences in `backend/src` DML (distinct statements): `RETURNING` 24, upsert 12,
`FOR UPDATE` 6, advisory lock 6, `ILIKE` 5, `= ANY(array)` 4, `DISTINCT ON` 1.

Refused by design, DML statements, distinct statements each: `pg_advisory_xact_lock` 6,
`FOR UPDATE/SHARE` 6, `= ANY(array)` 5, `array_agg` 2, `pg_backend_pid` 2, `set_config` 1,
`ARRAY[..]` 1. Note the sum exceeds 20 because one statement can carry several reasons.

## Results: DDL and other

| Group | n | exact | emulated | conditional | unmapped | refused | unparsed |
|---|---|---|---|---|---|---|---|
| DDL, auth-framework (all files) | 31 | 0 | 19 | 11 | 1 | 0 | 0 |
| DDL, small-app `migrations/*.sql` | 5 | 0 | 2 | 3 | 0 | 0 | 0 |
| DDL, large-app migrations + src + tests | 43 | 0 | 31 | 6 | 1 | 5 | 0 |
| Other (triggers, functions, DO, SHOW, COPY, SET), all corpora | 42 | 6 | 0 | 0 | 1 | 29 | 6 |

DDL is never "exact" by construction in the rule table (types and defaults differ per engine), so
"emulated" there means "needs a per-engine migration, as section 3.3 says". Triggers and
functions: 11 `trigger` and 16 `function/procedure` statements across auth-framework and large-app
`.sql` files combined (not comparable to `DESIGN-KINDS`' large-app-only 11 triggers and 14 functions,
which I did not recount separately); all refused, as designed.

## Untranslatable statements, by tier

Complete list with `file:line` and reason: `measure/untranslatable-by-tier.txt` (regenerate with
`python3 -I measure/report.py`). Summary of refused-by-design (all corpora, all kinds): trigger 11,
function/procedure 16, `SHOW` 8, advisory lock 6, row lock 6, array operand 5, `COPY` 2.

**Unparsed (6): five real SQL texts that are parser limits or templated fragments, and one prose literal:**
`DROP DATABASE .. WITH (FORCE)` (1), `CREATE EVENT TRIGGER` (2, would be refused anyway), an `UPDATE`
with a `format!` fragment in the SET list (1), adjacent string literals across lines in a
`COMMENT ON` (1, `0004_oauth_identity_claim_type.sql`), and one prose literal that matched the SQL
filter (`set <ENV_VAR> to ...`, small-app `tests/gguf_bpe_tokenizer_fidelity.rs:450`).

## The oracle finding: statement translation alone does not reproduce the SQLite backend

The question was whether auth-framework's hand-written SQLite SQL matches what UniSQL would emit from the
Postgres SQL. By reading both at `4582a64f`: **it does not, and the difference is schema and value
representation, which a statement translator cannot supply.**

- `auth_tokens`: Postgres inserts 13 columns (`token_id, user_id, access_token, refresh_token,
  token_type, expires_at, scopes, issued_at, auth_method, subject, issuer, client_id, metadata`);
  SQLite inserts 5 (`token_id, user_id, access_token, data, expires_at`) and keeps the rest in one
  `data` text blob. A translation of the Postgres `INSERT` names columns SQLite's table does not have.
- `expires_at`: `TIMESTAMPTZ` nullable (Postgres) vs `INTEGER NOT NULL` unix seconds (SQLite,
  `sqlite.rs` `token.expires_at.timestamp()`). `NOW()` is a **bound parameter** in the SQLite SQL, not
  a function: the application supplies the clock.
- Behavioural difference (a finding about the two backends, not UniSQL): active-session count is
  `expires_at IS NULL OR expires_at > NOW()` in Postgres (`postgres.rs:574`) and `expires_at > ?` in
  SQLite (`sqlite.rs:228`); equal only because SQLite's column is `NOT NULL`.
- What does match: the upsert shape (`ON CONFLICT (k) DO UPDATE SET c = EXCLUDED.c` vs
  `excluded.c`, 3 statements), single-row `SELECT/DELETE` by key, and the `LIKE` + ordered key scan.

Consequence for the design: the portable unit is the statement **plus a column/type codec** per target
(timestamp as text vs epoch, JSONB as text, row-as-blob). That is a Conditional-tier mapping the
caller must declare; DESIGN.md section 3 already has the tier, but "Types" needs to be a first-class
input to translation and not a footnote. The 19 Postgres DML statements are all translatable *as
text*; whether auth-framework should share one schema across backends is the project's call.

## What this does and does not establish

- Establishes: parse coverage of a Postgres corpus by `sqlparser` 0.63.0, and which constructs occur,
  with counts at the SHAs above.
- Does **not** establish: that any "exact" or "emulated" statement behaves the same on SQLite (nothing
  was run); that `unmapped` functions are untranslatable; that the prose filter missed nothing outside
  the 51 read; the 119 runtime vs 243 compile-time split (my `src` count of 339 is by literal, not by
  `query!` vs `query(`; it neither confirms nor refutes `DESIGN-KINDS` 362).
- The rule table was written by the same author as the tiers in DESIGN.md 3.1. **A shared author
  defeats agreement as evidence**; the differential layer is the independent check.
