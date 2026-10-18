# unisql-macro-core

What UniSQL's compile-time macros do, as a plain library: check a literal statement and translate it for
each named target, with no `proc_macro` types, so another proc-macro crate (PersistAnt's) can wrap it with
its own checks.

Part of [UniSQL](https://github.com/ThinkersJournal/unisql) (pronounced "unicycle"). Design:
`docs/MACROS.md`. Licensed under MIT OR Apache-2.0.
