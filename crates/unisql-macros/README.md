# unisql-macros

`sql!`: a literal SQL statement is checked and translated for each named target when your crate is built.
An untranslatable statement, a syntax error or a skipped `$n` placeholder is a compile error; nothing is
parsed at run time. Expands to a `const` `unisql_embed::Query`, so depend on `unisql-embed` too.

```rust
const UPSERT: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [sqlite, mysql],
    allow = [upsert_any_unique_key],
    "INSERT INTO kv (k, v) VALUES ($1, $2) ON CONFLICT (k) DO UPDATE SET v = EXCLUDED.v"
);
```

Part of [UniSQL](https://github.com/ThinkersJournal/unisql) (pronounced "unicycle"). Design:
`docs/MACROS.md`. Licensed under MIT OR Apache-2.0.
