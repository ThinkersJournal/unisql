# Compile-time macros: design

Written 2026-10-18 at origin/main a5cb56e (0.24.11). Status: **built in 0.24.14 to 0.24.18 (slices 1 to 5, section 8); the PersistAnt catalog `Checker` is theirs**. This is the
"spike" that `docs/DESIGN.md` section 6, option 3 left open, now taken up: translate a literal
statement at build time and fail the build on what cannot be translated, with no database connection.

## 1. Goal and non-goals

A literal SQL string in Rust source is, today, parsed and translated when the program runs, so a typo,
a placeholder miscount or a construct a target refuses is found by a test or in production. The macro
moves every check that needs **only the text, the options and (optionally) a schema snapshot** to
`cargo build`:

1. **Syntax**: the text parses in its declared source dialect.
2. **Placeholders**: `$1..$n` are contiguous (no `$1, $3`), their count is a compile-time constant, and
   an argument list, when given, has exactly that many entries.
3. **Translation**: for every named target, the statement translates, and the per-target text,
   `param_order` and `param_bindings` are embedded as constants. The runtime does no parsing.
4. **Refusal is a build error**, naming the target, the leaf id, the reason, and the allowance or
   alternative the report offers. There is no best-effort mode: a `Conditional`/`Emulated` construct
   builds only when the call site grants its allowance by name (`Options` is the same type the
   library uses).
5. **Needs assertions**: the statement's construct ids (the `analyse` set) are exposed as a constant and
   a call may assert them (`requires`, `forbids`), so a project can pin "this query stays portable".

**Not done, stated plainly:** no semantic typing of PostgreSQL without a catalog (no result-column types,
no "does this column exist" without a schema), no execution, no DDL (ROADMAP "Not planned here"). A
typed record per target stays out of reach exactly as `DESIGN.md` section 6.4 says. Catalog-based
checks (unknown table or column, `INSERT` arity, column classes) belong to PersistAnt (section 7); UniSQL
supplies the hook, and a DDL-derived `Schema` for the checks that need only names.

## 2. Crates

Three new crates, same version as the rest (one number per project), none published before they work:

| Crate | Kind | Holds |
|---|---|---|
| `unisql-macro-core` | lib | Parses macro input, runs `unisql::translate`/`analyse_with`, builds an `Expansion` (per-target SQL, params, construct ids, diagnostics). Plain Rust, unit-testable, **no `proc_macro` types** beyond `proc-macro2`. Exposes a `Checker` trait (section 7). |
| `unisql-macros` | proc-macro | `sql!` (and later siblings). A thin wrapper: tokens in, `unisql-macro-core`, tokens out. |
| `unisql-embed` | lib, no dependencies | The value a macro expands to (`Query`, section 3). A proc-macro crate cannot export types, so these live here. The expansion path is `::unisql_embed` by default and overridable with `crate = path`, so a re-exporting crate (PersistAnt) names its own. |

Why `unisql-macro-core` is separate: **a proc-macro crate cannot be extended**, only wrapped. PersistAnt
writes its own thin proc-macro that calls `unisql-macro-core` with a catalog `Checker`; that is how it
"re-exports it and adds the catalog-based checks" without forking.

`unisql` (the facade) does **not** depend on the macro crates: `unisql-macros` depends on `unisql`
(it translates), so the other direction would be a cycle. Users depend on `unisql-macros` and
`unisql-embed` (PersistAnt's users on neither: it re-exports both). Names `unisql-macros`,
`unisql-macro-core`, `unisql-embed` were free on crates.io on 2026-10-18 (404); claiming each with a
0.0.0 placeholder is the PM's step, as `PUBLISHING.md` section 1 records for the existing six.

## 3. Surface

```rust
let q = unisql_macros::sql!(
    source = postgres,                       // required: no dialect guessing (DESIGN 0.1 item 1)
    targets = [sqlite, mysql],               // see section 4
    allow = [text_collation],                // Allowance names, as `Allowance::name()` prints them
    requires = ["upsert"], forbids = ["fn:md5"],   // optional needs assertions
    "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v"
);
let r = q.for_target(Target::Sqlite).unwrap();   // `Option<&Rendered>`: no parse, no allocation
```

`Query` (in `unisql-embed`) is a `const`-constructible struct: `PARAMS: u32`, `CONSTRUCTS: &[&str]`, and
per embedded target a `Rendered { sql: &'static str, param_order: &'static [u32], param_bindings:
&'static [ParamBinding] }`. A target that was not embedded gives `None`; making that a compile error is open question 9.
`ParamBinding` is mirrored in `unisql-embed` and checked equal to `unisql_core::ParamBinding` by a test
in the macro crate, so `unisql-embed` stays dependency-free.

Argument checking is a second form, `sql!(.., "…", a, b)`, which yields `(Query, (A, B))` after checking
`PARAMS == 2`; it binds nothing (drivers are the caller's). The first slice ships the bare form only.

## 4. Choosing targets

Cargo features cannot choose them alone: features unify across a workspace, so any dependency enabling
`mysql` would add a MySQL translation (and its refusals) to every call in the build. Decision:

- **The call site names its targets** (`targets = [...]`). That list is the contract and is what a
  reader of the code sees.
- Cargo features of `unisql-macros` (`postgres`, `sqlite`, `mysql`, `mariadb`) that would drop dialect
  crates from the macro's build are **not planned**. Measured 2026-10-20 at 0.24.18 (clean dev build, the
  adapter already built): `unisql-dialect-sqlite` 2.8 s, `unisql-dialect-postgres` 1.7 s,
  `unisql-dialect-mysql` 2.6 s, against 96 s for the whole macro crate, of which `sqlparser` is most. Gating
  would save about 5 s (5%) and put a "this dialect is not compiled in" state into `unisql`'s error
  surface and every consumer's feature matrix. Every target is always available. Revisit only if a
  measurement shows the dialect crates growing to a real share.
- Omitting `targets` is an error in slice 1 (no hidden set). A project-wide default can come later as
  a `const`-evaluated list in the project's own macro wrapper, which is a call into `unisql-macro-core`
  and needs nothing here.

## 5. Errors and warnings

- **Spans.** Stable proc macros cannot point inside a string literal (`Literal::subspan` is
  unstable), so the error span is the literal, and the message carries the source line, a caret under the
  offending byte, and `line:col` when the reader provides a position. Today `ReadError::Syntax` carries
  only the parser's text, so a position is a slice-2 change to the adapter (parser errors do have one),
  not assumed here. Option and placeholder errors point at the macro argument that is wrong.
- **No warnings.** Stable proc macros cannot emit warnings. The policy needs none: `Exact` builds
  silently; `Conditional`/`Emulated` build **only** with the allowance named at the call site (else
  an error naming it); a refusal is an error. A construct that deserves a note is therefore always an
  explicit `allow`, which is itself the reviewed record.
- **Every error names** target, leaf id, reason, and what would allow it, taken from `Untranslatable`
  and `Report`, so the macro never rewrites a message the library already phrases.

## 6. Options, schema, cost, tooling

- **Options at the call site.** `allow = [..]` takes `Allowance` names; an unknown name is an error
  listing the known ones. `Options` has no other knobs today, and the macro adds none.
- **Schema (database-free).** `schema = "path/to/schema.sql"` (DDL in the source dialect; `schema_ddl = ".."` is the same inline) feeds
  `schema_from_ddl`, so numeric-column comparisons stop demanding `text_collation` and a sole unique key
  can serve an upsert without a target. The path is relative to `CARGO_MANIFEST_DIR`, read at expansion,
  and the expansion also emits `const _: &str = include_str!(..)` so **cargo rebuilds when the file
  changes** (a proc macro has no stable way to register a dependency otherwise).
- **Cost, measured 2026-10-20 at 0.24.17 on a 32-core Windows 11 machine, unoptimised (what a
  proc-macro is built with by default).** A call translating one upsert for four targets takes
  about 1.4 ms (`tests/cost.rs`). Reading a schema takes about 9 ms for 20 tables and 90 ms for 200
  (linear), so a crate with 300 `sql!` calls naming one 200-table schema would spend about 27 s
  re-reading it: the macro keeps a parsed schema for the life of the compiler process, keyed by
  (kind, dialect, text), so an edited file is a new key and only successful reads are kept. A clean
  dev build of `unisql-macros` takes 96 s against 23 s for a proc-macro crate with only
  `syn`/`quote`/`proc-macro2`; `sqlparser` and the UniSQL crates are the other ~70 s, paid once per
  clean build (the 32 cores hide part of it). Statements are not cached: one occurrence is expanded once
  per compiler run, and identical statements cost 1.4 ms.
- **rust-analyzer.** It runs the macro, so errors appear as diagnostics. The expansion must be pure
  (same tokens and files, same output; no environment reads other than `CARGO_MANIFEST_DIR`, no
  network, no clock), which also keeps builds reproducible. Failing macros return `compile_error!` tokens
  rather than panicking, so analysis of the surrounding file survives.
- **MSRV** follows the workspace (`rust-version` 1.94); `unisql-embed` is `#![no_std]`.

## 7. How PersistAnt layers the catalog checks

PersistAnt owns the catalog reader (ROADMAP "Schema, what is left"). The seam is a trait in
`unisql-macro-core`, called with the **parsed source statement** (`unisql::ast`) and the span-bearing
context, before translation:

```rust
pub trait Checker {
    /// Return every problem found; each becomes a compile error at the call site.
    fn check(&self, stmt: &ast::Statement, ctx: &CheckContext) -> Vec<Problem>;
}
```

PersistAnt's own proc-macro crate (re-exported under its name) loads a **schema snapshot file**
(its format, produced by its catalog reader, committed beside the code, tracked with `include_str!` as in
section 6) and implements `Checker`: table and column existence, `INSERT` arity, column classes, later
types. UniSQL ships one reference `Checker` over `Schema` (names only), used by `schema = ` and as
the worked example. The trait is **unstable until PersistAnt has built against it** (0.x, a break is a
new second number); the agreed first consumer shapes it, so it is settled with that lane in slice 4 and
not fixed here.

## 8. Slices (red first; each slice is one PR, with a CHANGELOG entry and a version bump)

0. This document and the ROADMAP rows.
1. `unisql-macro-core` and `unisql-embed`: `Expansion` from tokens-free inputs; unit tests red first.
2. `unisql-macros`: bare `sql!`, `source`, `targets`, no catalog, PostgreSQL to SQLite/MySQL. A
   **`trybuild` compile-fail suite** (one `.rs` and one `.stderr` per failure: syntax error, `$1,$3`
   gap, unknown target, target feature off, refused construct, unallowed conditional) plus
   compile-pass cases whose embedded text is asserted equal to `unisql::translate`.
   `trybuild` and `.stderr` files are rustc-version sensitive: CI pins the toolchain for that test.
3. `allow`, `requires`, `forbids`, `CONSTRUCTS`; the argument-checking form.
4. `schema = ` and the reference `Checker`; the `Checker` seam agreed with PersistAnt.
5. Caching and a recorded build-cost measurement; positions in syntax errors. **Done at 0.24.18:** the
   parser's `Line: n, Column: m` is parsed out of the message (`ReadError::Syntax` is unchanged, so
   nothing breaks) and shown as the offending line with a caret; an error with no position is left as is.

Every slice is checked by the compile-fail suite; a check that cannot be written as a compile-fail case
is not considered done.

## 9. Open questions (for the PM or CireSnave, not decided here)

- Whether `unisql-embed` should carry a `Target` enum or only string names, and whether asking for a target
  that was not embedded can be a compile error (an enum plus per-target accessors would, and couples the crate to the dialect list).
- Whether a project-wide default `targets` list is worth a third crate or stays a wrapper macro.
- A `query_as`-style typed record: only with a catalog, so PersistAnt's side; out of UniSQL.
