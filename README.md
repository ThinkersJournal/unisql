<p align="center">
  <img src="assets/Unicycle.svg" alt="UniSQL: a unicycle" width="140">
</p>

# UniSQL

Write SQL in one dialect; get it back in another. Pronounced "unicycle".

**Status: design phase, unreleased.** Nothing is published, no crate name is reserved, and the
repository location and ownership are undecided. The crates in this workspace are
`publish = false`.

```rust
use unisql::{translate, Options};

let t = translate("SELECT id FROM users WHERE id = $1", "postgres", "sqlite", &Options::default())?;
assert_eq!(t.sql, r#"SELECT "id" FROM "users" WHERE "id" = ?1"#);
// t.report says, per construct, whether the target gave it exactly, by emulation, or conditionally.
```

What it does and does not promise, in one paragraph: a statement is translated only when the target
can give it the source's meaning. Where it is the same only under an assumption (text collation,
`now()` as text, lenient casts), you must name that assumption in `Options`, or you get an error and
**no text**. Where no equivalent exists, you get an error naming the construct and what to do
instead. There is no best-effort mode.

- `docs/DESIGN.md`: scope, approach, tiers, decisions.
- `docs/MEASUREMENT.md`: how much of three real codebases' SQL translates (rule-table counts).
- `docs/DIFFERENTIAL-postgres-sqlite.txt`: the latest differential run against real engines.
- `docs/LICENCES.md`: licences of the projects we learn from, read from their files.

## Layout

| Crate | Role |
|---|---|
| `unisql` | the `translate` entry point; the only crate most callers need |
| `unisql-core` | the IR, tiers, report, shared printer; knows no dialect |
| `unisql-dialect-postgres` | PostgreSQL reader and target |
| `unisql-dialect-sqlite` | SQLite target |
| `unisql-differential` | tests that run both engines and compare results |

## Running the differential tests

They need a real PostgreSQL (and fail if it is unreachable; they are never skipped):

```
docker run -d --name unisql-pg -e POSTGRES_PASSWORD=unisql \
  -e POSTGRES_INITDB_ARGS="--locale=C --encoding=UTF8" -p 127.0.0.1:55432:5432 postgres:16
cargo test -p unisql-differential
```

`UNISQL_PG_URL` overrides the connection string. The `--locale=C` matters: it is what makes
PostgreSQL's text ordering match SQLite's byte order, which is the premise of the `text_collation`
allowance.

## Licence

`MIT OR Apache-2.0` for the code. The logo in `assets/` is "Unicycle" by GDJ from
[Openclipart](https://openclipart.org/detail/221146/unicycle) (public domain per the site; see
`docs/LICENCES.md`).
