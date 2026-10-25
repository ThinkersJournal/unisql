<img src="../assets/Unicycle-Small.png" alt="UniSQL unicycle logo" align="right" width="81">

# UniSQL: one SQL for every database

Design note, written 2026-10-12 and amended since. It records what the project is for, the shape of
the solution and why, what is deliberately out of scope, and what is still open. Pronounced
"unicycle".

## 0. What this is, and how far to trust it

**The project:** an SQL abstraction layer that lets a program write SQL in any dialect and have it
translated for whatever dialect the database in use needs, with as close to full portability across
SQL-speaking databases as can be measured. It works with PersistAnt's database interface (a
separate project) but does not depend on it.

**Provenance of facts.** Facts about PersistAnt come from its `docs/DESIGN-KINDS.md` (read 2026-10-12,
written 2026-10-11); its sections are cited as `DESIGN-KINDS §n`. Facts about prior art come from the
READMEs and doc pages named in section 4, fetched 2026-10-12 through a summarising fetch tool, so
every sentence quoted from them is **the tool's rendering of the page, not the page's bytes**; where
that matters it is marked. Crate versions, licences and MSRVs come from the crates.io API on
2026-10-12. Anything not established is under **UNVERIFIED** (section 11) and marked inline.

## 0.1 Direction (decided 2026-10-12; supersedes any conflicting text below)

1. **Each statement (or each project) declares the dialect it was written in.** UniSQL parses with
   that dialect and emits any target. Guessing the dialect from text is not offered (section 1).
2. **`sqlparser` is useful but replaceable.** It sits behind an adapter, so the project is not locked
   to it forever. The IR is ours and owns no sqlparser type in its public surface.
3. **SQLGlot is the primary reference** for dialect tables and generator structure: it does what this
   project does, and its Python reads as pseudocode. jOOQ has a similar goal but is Java, so it is
   read for ideas. Diesel and sqlx are different layers, integration targets rather than prior art
   to avoid.
4. **Scope is comprehensive, not consumer-driven.** Unlike PersistAnt's rule that "a capability is
   added only when a consumer needs it", UniSQL aims at broad coverage of SQL dialects for parsing
   into the IR and emitting from it. The first *shipped* source/target set (D1, D2: PostgreSQL and
   SQLite) is a sequencing decision, not a boundary. PersistAnt's `rel.*` ids stay gated by
   PersistAnt (section 5).

## 1. Scope: one canonical source dialect, or any?

"Write any SQL dialect" admits two readings, and they cost very differently.

| Reading | Meaning | Cost |
|---|---|---|
| **R1: canonical** | Every program writes one defined dialect (say, a PostgreSQL subset) and UniSQL emits the others. | One parser configuration; the translation matrix is N targets. |
| **R2: declared source** | Each statement (or each project) *declares* the dialect it was written in; UniSQL parses with that dialect and emits any target. | Matrix is S sources by N targets, but only through a shared normalised form, so the work is S parsers plus N emitters, not S times N. |
| **R3: undeclared** | Guess the dialect from the text. | Not offered: the same text means different things in different dialects (`"x"`, `||`, `LIMIT`), so a guess changes meaning silently. SQLGlot also requires both dialects to be named (section 4, row 2). |

**Decided: R2 in the architecture, PostgreSQL as the first shipped source.** The API carries a
`source` dialect parameter from day one (so "any dialect" is never a breaking change). PostgreSQL
came first because the programs measured in `DESIGN-KINDS §1` write PostgreSQL SQL. The first targets
are PostgreSQL (identity, used as the control) and SQLite, because auth-framework already has both
backends by hand and so supplies a ready-made oracle (section 7).

The design supports "per project" and "per statement" declaration alike, because the dialect is an
argument, not a global.

## 2. Approach

```
 source text + source dialect
        │  parse  (sqlparser; syntax only)
        ▼
   source AST ──► normalise ──► UniSQL IR (typed, dialect-neutral nodes + "needs" set)
                                   │  capability check against target
                                   │      ├─ all met        ─► emit
                                   │      ├─ emulable       ─► rewrite, record "emulated: <why>"
                                   │      └─ not expressible ─► Err(Untranslatable{..})   (never best-effort)
                                   ▼
                          target emitter ─► SQL text + parameter order + report
```

1. **Parse with an existing parser; do not write one.** `sqlparser` 0.63.0 (Apache-2.0, crates.io
   2026-09-13) names sixteen dialect structs (AnsiDialect, BigQueryDialect, ClickHouseDialect,
   DatabricksDialect, DuckDbDialect, GenericDialect, HiveDialect, MsSqlDialect, MySqlDialect,
   OracleDialect, PostgreSqlDialect, RedshiftSqlDialect, SQLiteDialect, SnowflakeDialect,
   SparkSqlDialect, TeradataDialect) and ships `Visit`/`VisitMut`/`Visitor`/`VisitorMut` for walking and
   rewriting its AST. Its README says it "tries to avoid applying any SQL semantics" and that "different
   SQL with seemingly similar semantics are represented with the same AST" (fetch-tool rendering).
   **Both facts shape the design:** parsing is solved, meaning is not, and the parser will not tell
   us when two syntaxes differ in meaning.
2. **Emission is ours.** Neither the README nor the docs.rs dialect page says sqlparser can emit for a
   *different* target dialect (both silent; its `Display` round-trips the source, per its README).
   So per-target quoting, placeholders (`$1` vs `?`), `LIMIT`/`OFFSET`/`FETCH`, boolean and
   string-concat forms, upsert, `RETURNING`, and date functions are UniSQL's own, written and tested
   per target. This is the bulk of the work and the part that never finishes.
3. **A normalised IR, not the raw AST, is what rewrites operate on.** Reason: the AST encodes syntax
   from one dialect; a rule such as "`ILIKE` on a target without it" is easier to state and test
   against one IR node than against four spellings. The rule that matters is that **a construct not
   in the IR is an error, not a pass-through.**
4. **Every translation returns a report, not just a string:** the constructs used, the capability each
   needed, and for each whether the target gave it *natively*, *emulated* (with the stated difference),
   or *refused*. A caller can log it, assert on it in CI, or require "native only".
5. **Per-dialect capability tests, in three layers.**
   - *Golden:* source text in, expected target text out, checked in. Catches emitter regressions only.
   - *Differential against real engines:* run the source statement on a source-dialect engine and
     the translated statement on the target engine over the same fixture data and compare result
     sets, ordering and error class. This is the only layer that tests *meaning*. Engines in CI:
     PostgreSQL, MySQL and MariaDB as service containers, SQLite in-process.
   - *Refusal tests:* for every construct classified untranslatable (section 3), a test that the
     error is raised, carries the reason, and that no SQL text is produced. A suite of passing
     translations satisfies half the testing rule; this layer is the half that gets skipped.
6. **Capability matrix as a checked-in artifact.** `(construct, source, target, engine version) ->
   native | emulated | refused | untested`, generated by the differential layer. This is the same
   shape as PersistAnt's `Proven | Declared | Absent` status (`DESIGN-KINDS §2.1`), deliberately.
   A cell that no test exercised is `untested`, never `native`.

## 2.1 Dialects are modules

Decided: SQL dialect support is modular so that adding a dialect is simple-ish. Design consequences:

1. **The core knows no dialect.** The IR, the need/capability vocabulary, the tiers, the report and the
   refusal rule live in a `unisql-core` crate with no dialect-specific text in it.
2. **A dialect is one unit that supplies up to four independent parts**, each optional so a dialect can
   be target-only or source-only at first:
   | Part | What it supplies | Test it must pass |
   |---|---|---|
   | **Reader** (source side) | text -> IR, via a parser adapter (sqlparser today, replaceable, section 0.1) plus the dialect's *normalisation* rules into IR nodes | round-trip corpus; every construct it accepts lands in the IR or errors with a reason |
   | **Writer** (target side) | IR -> text: identifier quoting, placeholder style, literal forms, `LIMIT/OFFSET/FETCH` shape, function-name and operator tables, type names | golden files per construct |
   | **Capability table** | for each construct id: `native | emulated(rewrite id) | absent`, with the minimum engine version | the differential layer fills `evidence`; an entry without a test is `declared`, never `proven` (section 5.4) |
   | **Rewrites** | named, individually testable IR-to-IR transformations the table refers to (`distinct_on_via_window`, `string_agg_to_group_concat`) | each rewrite has a before/after case and a refusal case |
3. **Data over code wherever it can be.** Quoting, placeholder form, function-name maps, type maps and
   the capability table are declarative data a contributor edits without writing Rust (the SQLGlot
   structure of one generator class per dialect with override tables is the model; its licence
   step in section 4 applies before anything is borrowed). Rust is needed only for a rewrite that is
   structural or for a reader that needs a hand-written parser.
4. **Registration is by crate and feature, not by editing the core.** A dialect crate (`unisql-dialect-*`)
   exposes one registration entry point; `unisql` re-exports a set behind cargo features
   (`postgres`, `sqlite`, `mysql`, ...). Third parties can add a dialect without a PR to this repo.
   Dialects may **inherit** (MariaDB over MySQL, Redshift over Postgres) as a table of overrides on a
   parent, with the parent's tests re-run against the child.
5. **A dialect is declared with a version range.** The capability table is keyed by engine version, so
   "SQLite >= 3.35 has `RETURNING`" is a row, not a code branch.
6. **Adding a dialect is a checklist, enforced by the test kit, not by review:** (a) a quoting and
   placeholder file; (b) a type map; (c) a function map; (d) a capability table with every construct id
   in the core's list present (an absent id is a build error, so a new construct in the core forces every
   dialect to say something); (e) a golden file per construct; (f) a differential run if an engine is
   available, otherwise every row stays `declared`.
7. **Capability ids vs construct ids (section 5.5):** adding a dialect never adds a PersistAnt `rel.*`
   id. It adds rows.

**Not claimed:** that data files suffice for every dialect (Oracle's empty-string-as-`NULL`,
SQL Server's `TOP` and `OUTPUT`, and procedural dialects will need Rust rewrites), or that the first
two dialects found the right trait boundaries. Expect to redo the boundary once, at the third dialect
(MySQL), and keep the first two small enough that this is cheap.

## 2.2 The public API shape

Decided: UniSQL takes a string of SQL, a source dialect and a destination dialect and produces the SQL
translated to the destination dialect. It does not care about any other part of the stack (sqlx, for
example). Four refinements, none of which add a dependency on any other layer:

1. **The result is `Result<Translation, Error>`, not a bare string.** A refusal must produce no text
   (section 3), and the report is how a caller sees emulation. A convenience `translate_str` that
   returns only the text is fine as long as it still errors on refusal.
2. **An options argument carries the named allowances** (the Conditional tier). No allowance, no text.
3. **The result carries the parameter order**, because the text alone cannot be bound correctly when
   the target uses positional `?` and the source used `$2, $1` or reused `$1`.
4. **The result carries a per-parameter binding** (`Translation::param_bindings`): some parameters
   change *shape* between engines. `x = ANY($1)` with an array on PostgreSQL becomes
   `x IN (SELECT value FROM json_each(?1))` on SQLite and `JSON_CONTAINS(?, JSON_ARRAY(x))` on
   MySQL/MariaDB, where the caller must bind the array as JSON text.
   `ParamBinding::{Scalar, NativeArray, JsonArrayText}` says which, per source parameter. A target
   with no codec **refuses** (`array_param`); a parameter used both as an array and as a single value
   is refused. The array forms carry the `array_null_semantics` allowance: a NULL array, NULL
   elements or a NULL operand are definite on the target and unknown on the source.

Dialects are named at runtime (`"postgres"`, `"sqlite"`), looked up in a registry of the dialect
modules compiled in (section 2.1). Integrations (sqlx, Diesel, PersistAnt) are separate optional
crates that call this function; the core never depends on them.

## 3. The honest limit of "full portability"

"As close as possible to full portability" is the right phrase. The layer's deliverable is **a
measured boundary, not an absence of one.** Every construct falls in exactly one tier:

| Tier | Meaning | Caller sees |
|---|---|---|
| **Exact** | Same meaning on source and target by construction and by test. | Plain translation. |
| **Emulated** | Target lacks the construct; an equivalent form exists. Cost or edge cases are stated. | Translation plus a report entry naming the difference. |
| **Conditional** | Same meaning *only if* the caller accepts a stated assumption (for example "text comparison is case-sensitive"). | Error unless the call site carries an explicit, named allowance. |
| **Refused** | No equivalent that preserves meaning. | `Err(Untranslatable { construct, source, target, reason, alternatives })`. **No SQL text.** |

**The rule that makes the tiers meaningful: there is no "approximate" tier that applies by default.**
SQLGlot's documented default is the opposite: it "may emit a warning and will proceed to do a
best-effort translation", and only `unsupported_level` set to `RAISE` or `IMMEDIATE` raises
(fetch-tool rendering of its README). For a persistence layer under authentication tokens and
safety-relevant records that default is the wrong one. UniSQL's default is SQLGlot's strict mode, and
relaxing it is per-construct and named.

### 3.1 What is in each tier (a proposal; rows checked against the matrix files on 2026-10-25 say so, and the matrix files, not this table, are authoritative)

| Area | Examples | Tier, Postgres to SQLite | Why |
|---|---|---|---|
| Core DML | `SELECT`/`INSERT`/`UPDATE`/`DELETE`, joins, `GROUP BY`, `LIMIT/OFFSET`, `RETURNING` | Exact (`RETURNING`: leaves `returning`, `returning.update` native, run on SQLite 3.53.2; the 3.35 floor is not run) | |
| Upsert | `ON CONFLICT … DO UPDATE` | Exact, `… WHERE` form too (leaves `upsert`, `upsert_where` native, SQLite 3.53.2) | sea-query shows upsert is dialect-rendered: `ON DUPLICATE KEY UPDATE` on MySQL (fetch-tool rendering) |
| Placeholders, quoting | `$1`, `"ident"` vs `` `ident` `` vs `[ident]` | Exact | mechanical |
| Boolean, concat, casts | `true`, `||`, `::int` | `\|\|` native (`concat`); `true` and `::int` Conditional (allowances `boolean_as_integer`, `lenient_cast`) | rewritten forms, per target |
| `DISTINCT ON`, `FOR UPDATE OF`, `ILIKE` | | `DISTINCT ON` Emulated (window function; refused with a `*` projection); `ILIKE` Conditional (`ascii_only_case_folding`, `like_case_sensitivity_is_connection_setting`); row locks (`row_lock`, `row_lock.share`) **Refused** on SQLite | SQLite has no row locks (`DESIGN-KINDS §2.2`, `rel.row_lock` row) |
| Case/collation, `NULL` ordering, integer division, `NULL` in concatenation, empty-string-as-`NULL` on Oracle | | **Conditional** at best | same text, different result; silent drift is the failure to avoid |
| Types | `JSONB`, `TEXT[]`, `INET`, `TIMESTAMPTZ`, `NUMERIC` precision | `JSONB`/arrays Refused or Emulated-by-text; `TIMESTAMPTZ` Conditional | SQLite has storage classes, not these types |
| Transactions and isolation | `REPEATABLE READ READ ONLY`, savepoints, advisory locks | Isolation and advisory locks **Refused**; savepoints Exact | engine semantics, not syntax |
| DDL | `CREATE TABLE`, indexes | Exact for a stated subset; **expression and partial unique indexes**: Exact where both support them | |
| **Triggers, functions, procedural code** (PL/pgSQL, `CREATE FUNCTION`) | | **Refused**, always | the body is a different language per engine; translating it is a compiler project, and a wrong translation breaks the invariants the application relies on |
| **Extensions, engine-specific functions** | `COPY`, `set_config`, `hashtext`, catalog reads | **Refused** (a pass-through escape hatch exists, section 3.2) | no portable meaning |
| Locking semantics | `SKIP LOCKED`, `NOWAIT`, advisory locks | Refused where the target lacks them | a lock that silently does nothing is worse than an error |

### 3.2 The escape hatch, and why it is not a loophole

PersistAnt's Option C is a pass-through that gives "zero query portability" (`DESIGN-KINDS §4.2`).
UniSQL keeps it **as an explicit, typed tier of its own**: `raw(dialect = postgres, "…")`. A raw
fragment declares the one dialect it is valid for, becomes a *need* (`rel.dialect(postgres)`), and the
target refuses it at open time, not at first execution. So a program is portable exactly to the extent
it avoids `raw`, and **that extent is countable** by listing its raw fragments. Nothing is silently
passed through.

### 3.3 What "full portability" cannot mean, said plainly

- It cannot mean one schema and one trigger set across engines. DDL and triggers stay per-engine
  migrations (PersistAnt's migration ledger keeps sqlx's `_sqlx_migrations` shape, `DESIGN-KINDS §5`).
- It cannot mean identical performance or plans. Emulated forms can be slower, and the report says so.
- It cannot mean databases that are not SQL-speaking in the relevant sense (key-value stores), which
  is what PersistAnt's `kv.*` kind is for.
- A program with trigger and lock paths stays "PostgreSQL by declaration" for those paths. What UniSQL
  changes is the other side of that ledger: most compile-time statement sites in such programs are
  single-table CRUD (`DESIGN-KINDS §4.2` estimates roughly 90%; UniSQL has not measured it), and those
  become measurable as translatable or not (section 7).

## 4. Prior art to evaluate and credit

Every project below was read (README or doc page, 2026-10-12, through a summarising fetch). None was
cloned or built. Provenance rule: **no code or table from any of them enters this project without
tracing the file to its origin and keeping its licence and notice.** Ideas are credited here and in the
README.

| # | Project | What it is (as fetched) | What we take | What we do not | Licence |
|---|---|---|---|---|---|
| 1 | **sqlparser-rs** `sqlparser` 0.63.0 | Rust syntax parser with the sixteen dialects above, `VisitMut`, round-trip `Display`. Declines semantics. | The parser and AST, as a dependency (from crates.io). | Any emission for a different target (it does not claim it). | Apache-2.0 (crates.io API); compatible as a dependency of an `MIT OR Apache-2.0` crate; its notice travels with binaries. |
| 2 | **SQLGlot** (Python) | Parses to an AST and generates SQL, "over 30 dialects", 32 built-in listed; a transpiler "not a validator"; strict mode raises `UnsupportedError`. | Design: transpile as `read=`/`write=`, explicit dialects both sides, per-dialect generator tables as the *structure* of the work, the strict-mode idea as our default. | Python code; its best-effort default. | **MIT**, "Copyright (c) 2026 Toby Mao", byte-read at `5b949bd6` (docs/LICENCES.md). Derivation permitted; keep the notice with derived portions. `tests/fixtures/jsonpath` is separate BSD-2. |
| 3 | **sea-query** 1.0.2 | Rust query *builder* (not a parser): fluent AST rendered by `MysqlQueryBuilder`/`PostgresQueryBuilder`/`SqliteQueryBuilder`; `build()` returns SQL plus bound values. No documented unsupported-feature detection. | The per-backend rendering split and the quoting/placeholder conventions as a **reference for our emitters**. Evaluate using it *as* the emitter (D4). | Its construction API (we have text input). | `MIT OR Apache-2.0` (crates.io API, README). MSRV 1.88.0. |
| 4 | **jOOQ** (Java) | Models dialects as an enum; marks each API with `@Support` listing native dialects and noting emulation; has a SQL-to-SQL translating parser (named only in link text on the page fetched). | The `@Support`-style idea: **each construct carries the list of targets that give it natively and which emulate it**, which is our matrix. Credit the idea. | Java code and its dialect list. | **Apache-2.0** by notice, with a commercial alternative "for commercial database integrations"; byte-read at `e450fb50` (docs/LICENCES.md). Open-repo content derivable with notices; which dialects are open is not established. |
| 5 | **Diesel** 2.3.14 | `MultiConnection` derive combines Postgres/MySQL/SQLite into one enum connection, but "can only cover the common subset of all inner connection types"; non-core SQL types need hand-written impls per backend. | The warning: *common-subset-only* is the failure mode of "one type for all backends". Our answer is the tiers plus a per-target report, not the subset. | The ORM and schema DSL. | `MIT OR Apache-2.0`; MSRV 1.86.0 (crates.io API). |
| 6 | **sqlx** 0.9.0 | Does not parse SQL; "any syntax that the development DB accepts" works; `query!` needs `DATABASE_URL` to a DB of "the same kind" as production; `Any` proxies to a driver chosen at runtime by URL scheme; placeholders differ (`$1` vs `?`). | The driver and the checking mechanism (section 6). | `Any` as a portability story: it moves the driver choice to runtime but changes no SQL. | `MIT OR Apache-2.0`; MSRV 1.94.0 (crates.io API). |

**Standing of each row after the direction in section 0.1:**

| Project | Role in UniSQL | Condition |
|---|---|---|
| sqlparser-rs | Parser *adapter* for the first dialects, behind our IR boundary; may be replaced per dialect. | The IR exposes no sqlparser type; a dialect's parser is a trait object or feature, so a hand-written parser can take over without an IR change. The corpus measurement already shows its limits (6 real statements it cannot parse: `DO`, `CREATE EVENT TRIGGER`, `DROP DATABASE .. WITH (FORCE)`, adjacent-literal concatenation). |
| SQLGlot | **Primary information source**: dialect tables, per-dialect generator structure, the transformation catalogue, and its test corpus as a source of cases. | **Licence read 2026-10-12: MIT** (docs/LICENCES.md), so derivation is permitted. Keep the MIT notice and credit with every derived table or algorithm and trace each derived file to its SQLGlot source path and SHA. "Python as pseudocode" that follows the source closely is a derived work. |
| jOOQ | Read for the `@Support`-style capability annotations and its translating parser's behaviour; expect to glean concepts, not code. | **Apache-2.0** with a commercial alternative (docs/LICENCES.md): concepts free, code needs notices; do not assume a commercial-database dialect is in the open repo. |
| Diesel | **Integration target**: a `diesel` feature that routes a UniSQL-translated statement through `sql_query` or a custom `QueryFragment`. | Not designed yet; needs a spike. Its `MultiConnection` is the common-subset trap (row 5), so UniSQL feeds it statements and does not adopt it as the abstraction. |
| sqlx | **Integration target**: the prepare check (section 6.2) and a `sqlx` feature that hands translated text to `sqlx::query` per driver. | Bind order and placeholder style come from the report. |

**Gap in this table, stated:** none of these was read beyond README/doc-page level, so the claim "none
ships both a Rust parser and per-target emitters with a refusal-by-default policy" is **UNVERIFIED**;
it is only "not found in the six read". Searching for others (other Rust SQL-transpile crates,
`prql`-style languages, the dialect handling inside `datafusion`) comes before any further emitter work.

**Search, 2026-10-25 (crates.io search API, README level only; versions as listed that day).** Queries: "sql transpile",
"sql transpiler", "sql dialect translate", "sqlglot", "sql dialect convert", "postgres to sqlite sql". Found and read:
`sqlglot-rust` 0.10.33 and `polyglot-sql` 0.13.4 (30+ dialects each; their READMEs say nothing about what happens to a
construct the target lacks: 0 hits for refuse/unsupported/lossy in the first, validation options only in the second);
`sqlt` 0.3.5 (MySQL, MariaDB, PostgreSQL, T-SQL, SQLite; **best-effort by default**: "emits the closest equivalent and prints a
warning to stderr", `--strict` makes a warning fatal, README states RETURNING to MySQL as the example); `sqlquack` 0.1.0
(**fail-closed**, but DuckDB output only and "not a broad N x N dialect transpiler"); `sqlemit` 0.0.1 (emits from
`sqlparser` AST types per target; ClickHouse in its example; refusal policy not stated). Not read: `squonk` 3.0.0 (parser and
renderer, README only skimmed), `sqlout` 0.1.0 (its README returned AccessDenied), `papera` 0.2.0 (Trino/Redshift to DuckDB).
So the claim narrows to: **no crate found that is fail-closed across PostgreSQL, SQLite and MySQL/MariaDB**; the closest
(`sqlquack`) is fail-closed for one target and the multi-target ones read are best-effort or silent. This is still
README-level: the source of `sqlglot-rust` and `polyglot-sql` was not read, so their behaviour on an unsupported construct is open.

## 5. Composition with PersistAnt's `rel.*` vocabulary

PersistAnt is early in its development, so its relational bits can change where they do not mesh with
UniSQL's goals; the two projects agree such things directly, and neither's vocabulary is edited
unilaterally.

PersistAnt's Option C is a typed pass-through whose relational capability ids include
`rel.dialect(d, min)`, `rel.transaction`, `rel.isolation(level)`, `rel.savepoint`, `rel.foreign_key`,
`rel.check_constraint`, `rel.unique_index.expression`/`.partial`, `rel.trigger`,
`rel.row_lock(for_update)`/`.skip_locked`, `rel.advisory_lock`, `rel.upsert(where)`, `rel.json`/`.array`,
`rel.bulk_copy`, `rel.session_setting`, `rel.catalog` (`DESIGN-KINDS §2.2`). Its rule is "a capability
is added only when a consumer needs it". UniSQL composes by **making each statement derive its own
`Needs`**:

1. **Statement analysis produces a `rel.*` need set.** `FOR UPDATE` yields `rel.row_lock(for_update)`;
   `ON CONFLICT … DO UPDATE … WHERE` yields `rel.upsert(where)`; a `raw(postgres, …)` fragment
   yields `rel.dialect(postgres)`. That is the same list the target's report is built from.
2. **The program's statement catalogue is analysed at open**, so refusal is **at open with every unmet
   need named** (PersistAnt's gate: declare, report, refuse or open; `DESIGN-KINDS §2.1`), not at the
   first execution of an unusual query.
3. **`rel.dialect` keeps its meaning; routed statements get a new id.** `rel.dialect(d,min)` stays
   "this SQL text runs as-is on d; any other engine is refused", which is what a `raw(postgres, ..)`
   fragment yields. A statement routed through UniSQL yields a separate id, `rel.source_dialect(d)`:
   "written in d, translated; the target must translate every statement in my catalogue to
   non-Refused". Neither id is added to PersistAnt's list by this note.
4. **Matrix: one row schema, owned by PersistAnt; each repo checks in its own file.** The row is
   `subject_kind, subject, backend, version, config, enabled, reason, metadata` (PersistAnt
   `DESIGN-KINDS.md` section 2.4). UniSQL's file holds construct rows from its differential layer;
   PersistAnt's holds capability rows from its probes; the reader (the open-time gate in
   `persistant-relational`, which depends on `unisql`) merges them. Tier map: Exact -> native+proven,
   Emulated -> emulated+proven, Refused -> absent+proven. **Conditional is a call-site allowance and
   appears in the report, not as a matrix cell.** A missing row, or a `declared` one, is `untested`,
   never `native+proven`. Construct ids and capability ids are separate namespaces; the
   construct-to-capability mapping is what the report emits.
5. **Two vocabularies, two rules.** UniSQL's *construct* vocabulary is **comprehensive** (section
   0.1): every construct the supported dialects have is classified, with or without a user today.
   PersistAnt's *capability* ids stay consumer-gated by PersistAnt: a construct maps to a `rel.*` id
   only when PersistAnt has added that id. Candidates the mapping will want (`rel.distinct_on`,
   `rel.returning`, `rel.ilike`, `rel.window_functions`) are PersistAnt's to add; the measurement
   (docs/MEASUREMENT.md) supplies counts for that request. Until an id exists, the report carries the
   construct and the open-time gate treats it as unmapped, which refuses.
6. **Dependency direction.** `unisql` has no I/O and no database driver (it produces text and a
   report). `persistant-relational` depends on `unisql`; the reverse never. Capability ids are plain
   strings `rel.<name>[(arg[,arg])]` (args lowercase); the id column of `DESIGN-KINDS §2.2` is the
   normative list until a PersistAnt core crate ships it as data. `unisql` never imports
   `persistant-core`; it may take a **dev-dependency** on it for a drift test (core never depends on
   `unisql`, so no cycle). `unisql` emits no id outside the list except one marked `proposed:`.
   All `unisql` crates carry one version number.
7. **Transactions, pools, TLS, migrations, error classification stay in PersistAnt.** UniSQL is a
   function from `(text, source, target)` to `(text, report)` or an error. Keeping it that small is
   what keeps it testable.

## 6. How `sqlx::query!` compile-time checking survives

What is established (sqlx README, fetch-tool rendering, 2026-10-12):
- `query!` "connects to your development DB at compile time to have the database itself verify" the
  statement; `DATABASE_URL` "must be set at build time"; the database must be "the same kind (MySQL,
  Postgres, etc.) and have the same schema as the database you will be connecting to".
- Offline mode caches the analysis so builds need no live database.
- `Any` "can proxy to a database driver at runtime"; the `any` and `macros` features are separate.
  That the `query!` macros do **not** work against `Any` came from a third-party page and search
  summary, not sqlx's own docs: **UNVERIFIED**.

**Consequences, and the options:**

1. **A program loses nothing, because UniSQL is opt-in per statement.** Compile-time-checked sites keep
   calling `sqlx::query!` against `PgPool`, exactly as Option C leaves them. Portability is a property
   of the statements routed through UniSQL, not of the program. This is the property that makes the
   layer adoptable at all.
2. **Check the source dialect at compile time; verify the targets in CI.** A statement is written in
   the source dialect (Postgres). The sqlx macro checks it against a Postgres development database,
   as today. Targets are checked by the differential layer and by a **prepare check**: a build-time or
   test-time step that translates every statement in the program's catalogue and runs
   `PREPARE`/`EXPLAIN` on each target engine against the target schema. This does not give `query!`'s
   typed record for the target, but it does give "this statement is valid on SQLite against this
   schema" before shipping. **This is the recommended first mechanism: it needs no macro work.**
3. **A `unisql::query!` proc macro (spike, not a promise).** It would (a) translate for every enabled
   target at compile time, turning an *untranslatable* statement into a **compile error**, and (b)
   hand the source text to `sqlx::query!` for the source-dialect check. The unresolved part is the
   return type: sqlx's macro produces a record typed from the *source* engine and a query bound to
   that driver, so it cannot be executed on another driver directly. Whether the macro can be used
   purely for checking and a `FromRow`-style type for execution is **UNVERIFIED**; it is the spike
   in D3.
4. **What cannot survive, stated:** a *typed* record inferred from the target engine, for a statement
   that was translated. sqlx infers from whichever database `DATABASE_URL` names; no layer above it
   can make a Postgres-inferred record also be a SQLite-inferred record. Result-column typing
   across engines (an `INTEGER` that is `INT4` on one and 64-bit on another) is part of the
   Types row in section 3.1 and is a Conditional tier, not something macros fix.

## 7. First deliverable: a measurement, not an emitter

Before any translator is written, the build phase answers "how much of a real code base translates"
with a count, over a corpus that already exists, reported with the commit it was measured at:

1. **auth-framework's ~25 statements per backend** (23 call sites in `src/storage/postgres.rs`; the
   SQLite backends duplicate them). This is a ready oracle: the hand-written SQLite SQL either matches
   what UniSQL emits from the Postgres SQL, or the difference is a finding about one of them.
2. **A small application's five Postgres tables.**
3. **A large application's compile-time and runtime sites** as a *rejection corpus*: how many parse,
   how many translate Exact/Emulated, and for the rest the list of reasons, grouped. The expectation
   is that the trigger, advisory-lock, `COPY`, JSONB and `TEXT[]` sites refuse. **The count, not that
   expectation, is the deliverable.**

Control for each count: a statement known to translate and one known to refuse, run in the same
invocation.

## 8. Non-goals

1. Not an ORM, not a query builder, not a schema DSL. Text in, text and a report out.
2. Not a driver, a pool, a migration runner or a connection layer: PersistAnt (and sqlx) own those.
3. Not a validator of schema or types: "transpiler, not a validator" is SQLGlot's own framing, and
   sqlparser declines semantics for the same reason.
4. Not a translator for procedural code, triggers, functions, or engine extensions. They are refused
   or raw.
5. Not silent. No default that changes meaning without a report entry and a named allowance.
6. Not non-Rust in this phase.
7. Not performance tuning across engines.

## 9. Decisions

| # | Decision | Options considered | Outcome |
|---|---|---|---|
| D1 | Source scope (section 1) | R1 canonical only; R2 declared source with one shipped; R2 with several | **Decided: R2 architecture, PostgreSQL source first.** Sequencing only: the goal is comprehensive modular dialects (section 0.1) |
| D2 | First targets | Postgres + SQLite; add MySQL/MariaDB at once | **Decided: Postgres + SQLite first, sequencing only.** First slice = IR + Postgres source -> Postgres/SQLite emitters with differential tests against real engines; further dialects as modules (MySQL and MariaDB have since been added) |
| D3 | Compile-time story (section 6) | prepare-check only; prepare-check then a `unisql::query!` spike; macro first | Open: prepare-check first, macro as a spike afterwards |
| D4 | Emitter foundation | own emitters over sqlparser; sea-query as emitter; fork-and-extend a transpiler | Own emitters (the shipped choice); sea-query remains a reference |
| D5 | Capability ids shared with PersistAnt, and who owns the matrix file | `unisql` depends on `persistant-core`; plain-string spec in a third place; each owns its own | **Decided** (section 5.3-5.6): plain strings, PersistAnt owns the vocabulary and the row schema, each repo checks in its own matrix file |
| D6 | DDL in scope | DML only; DML + a stated DDL subset; full DDL | Decided with PersistAnt: schema management stays with the migration tool; DDL is read only to learn column types |
| D7 | Repo, name, owner | section 10 | Decided: `unisql`, published under `MIT OR Apache-2.0` |
| D8 | Parser independence | sqlparser as a hard dependency; sqlparser behind an adapter; own parsers from the start | **Decided:** adapter (section 0.1); own parser only per dialect where sqlparser is wrong or missing |
| D9 | Coverage target | consumer-driven (PersistAnt's rule); comprehensive across dialects | **Decided:** comprehensive (section 0.1); sequencing by D1/D2/D6, not by consumer |

## 10. Name, repo, licence

- **Name.** Checked on crates.io on 2026-10-12 through the read-only API: `unisql` and `uni-sql` were
  free (404; control: `unicycle` returned 200). The project is pronounced "unicycle", and a crate by
  that exact name exists with a different purpose, so searching for "unicycle" finds that one. This
  is a naming-quality point, not a blocker.
- **Licence:** `MIT OR Apache-2.0`, the dual licence common for Rust library crates. The dependency
  `sqlparser` is Apache-2.0 alone, which is compatible as a dependency.
- Repository policy: CI checks and branch protection that requires them before the first merged PR.

## 11. UNVERIFIED

- That nothing else in the Rust ecosystem already does per-target emission with refusal by default
  (section 4 gap). Only six projects were read, at README level.
- jOOQ's translating parser's actual behaviour (the page named it only in link text).
- Whether each cell in 3.1 holds on SQLite *at the minimum version we would require*. Checked
  2026-10-25 against the matrix files (SQLite 3.53.2 only): upsert and `… WHERE`, `RETURNING`, `DISTINCT ON`,
  `ILIKE`, row locks, concat, boolean and integer casts hold as the table now states. **Still not run:** older SQLite
  versions (the 3.35 `RETURNING` floor), and the rows with no leaf in the matrices (savepoints, isolation levels,
  advisory locks, type rows such as `INET`, DDL). The tiers for those remain a **proposal**.
- That `query!` does not work with `sqlx::Any` (third-party source), and whether a macro can use
  `sqlx::query!` for checking only (section 6.3).
- The "roughly 90% single-table CRUD" figure is PersistAnt's estimate in `DESIGN-KINDS §4.2`, not a
  UniSQL measurement.
- The summarising fetch tool's renderings of every external page above; none was read byte for byte.
