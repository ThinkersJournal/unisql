# unisql

Translate SQL text from one dialect to another. Pronounced "unicycle". No I/O, no driver: text in,
text and a report out.

```rust
use unisql::{translate, Options};

let t = translate("SELECT id FROM users WHERE id = $1", "postgres", "sqlite", &Options::default()).unwrap();
assert_eq!(t.sql, r#"SELECT "id" FROM "users" WHERE "id" = ?1"#);
// t.report says, per construct (leaf id), whether the target gave it exactly, by emulation, or
// conditionally, and which named allowance a conditional form needs.
```

- `translate(sql, source, target, &Options)`: PostgreSQL, SQLite, MySQL and MariaDB, any pair.
- `analyse` / `analyse_with`: what a statement needs, without choosing a target.
- `schema_from_ddl` and `Options::with_schema`: tell the translator the class and collation of
  base-table columns (for example from migrations) so comparisons on non-text columns need no
  collation assumption.

Every refusal and every divergence is named; there is no best-effort mode. See the
[repository](https://github.com/ThinkersJournal/unisql) for the design, the matrix files and the
differential tests that run every mapping against real engines. Licensed under MIT OR Apache-2.0.
