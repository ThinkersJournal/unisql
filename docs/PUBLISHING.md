# Publishing checklist and record (0.16.3 is published; later versions follow the same path)

Written 2026-10-14 at origin/main 2b663a0 (0.16.0). The manifests are ready (`publish = true` on the six crates,
`publish = false` on `unisql-differential`) and the six crates were published at 0.16.3 on 2026-10-14 from a clean archive of f95d207 (crates.io
read back: all six at 0.16.3, `yanked = false`). This file is the list of what must
be true first, what was checked, and what was found.

## 1. State of the names (crates.io API, read-only, 2026-10-14)

| Crate | On crates.io | Owner | Publish? |
|---|---|---|---|
| `unisql` | 0.0.0 placeholder, created 2026-10-12 | `ciresnave` | yes (the facade) |
| `unisql-core` | 0.0.0 placeholder | `ciresnave` | yes |
| `unisql-sqlparser-adapter` | 0.0.0 placeholder | `ciresnave` | yes |
| `unisql-dialect-postgres` | 0.0.0 placeholder | `ciresnave` | yes |
| `unisql-dialect-sqlite` | 0.0.0 placeholder | `ciresnave` | yes |
| `unisql-dialect-mysql` | 0.0.0 placeholder | `ciresnave` | yes |
| `unisql-differential` | not present (404) | none | **no**: `publish = false`, needs real engines, do not claim |

Nothing to claim. A real release publishes `0.x.y` over the placeholder from the same owner account.

## 2. What a release changes in the repository

1. In `[workspace.package]` set `publish = true` (do **not** just delete the key: the six crates say
   `publish.workspace = true`, and an undefined key fails manifest loading) and keep `publish = false`
   written out in `unisql-differential`.
2. Set the release version (one number for all crates) in `[workspace.package]`, the
   `[workspace.dependencies]` pins, the two member pins in `unisql-dialect-postgres` and
   `unisql-dialect-sqlite`, `Cargo.lock` and the `CHANGELOG.md` heading.
3. Update the status paragraph of `README.md` (it must say what is true at that tag).
4. Tag, then publish in dependency order (section 4).

## 2b. Dry run from a clean archive (2026-10-14, main d44db7a = 0.16.1, cargo 1.98.1)

`git archive HEAD` into an empty directory; `publish = true` set in `[workspace.package]` and
`publish = false` kept on `unisql-differential` (the edit of section 2, point 1); then

```
cargo publish --workspace --dry-run --locked      # exit 0
```

All six were packaged, **verified** (compiled from their packaged files) and stopped at "aborting
upload due to dry run", in this order: `unisql-core` (15 files, 134.9 KiB, 35.2 KiB compressed),
`unisql-sqlparser-adapter` (7, 93.2/24.4), `unisql-dialect-mysql` (9, 94.5/18.0),
`unisql-dialect-postgres` (10, 54.2/11.2), `unisql-dialect-sqlite` (9, 83.3/17.0), `unisql` (9,
55.7/16.7). A per-crate `cargo publish --dry-run -p <crate>` passes only for `unisql-core`: the others
cannot select `unisql-core ^0.16.1` (the registry has 0.0.0) until it is published, so `--workspace`
(which overlays the local packages) is the pre-check to use. One cosmetic warning: the facade excludes
`tests/leaf_contracts.rs` and cargo says it is ignoring that test.

## 3. Package contents (checked with `cargo package --list --offline -p <crate>` at 0.16.1)

| Check | Result |
|---|---|
| `description` on every crate | present (three were missing until this change) |
| `license = "MIT OR Apache-2.0"` and both licence texts inside each package | present (copies of the root files; `every_published_crate_carries_the_root_licence_texts` keeps them identical) |
| `readme` inside each package | present (`crates/<name>/README.md`) |
| `keywords` (5) and `categories` | `sql, dialect, translate, postgres, sqlite`; `database, parser-implementations` |
| `repository` | `https://github.com/ThinkersJournal/unisql` (private until the flip: **must be public** when the crates are, or the crates.io link is dead) |
| `rust-version` | 1.94, checked by the MSRV job (`cargo check` on the manifest's own version) |
| Files that read outside the package | the leaf-contract and licence-copy checks read `docs/` and sibling crates: they live in the unpublished `unisql-differential` crate (`tests/leaf_contracts.rs`), run by their own CI step, and no package refers to them |
| Matrix files | shipped inside the three dialect crates (`matrix-*.tsv`), as the data a gate reads |

## 4. Order and checks

Dependency order (each waits for the previous one to appear in the index):

1. `unisql-core`
2. `unisql-sqlparser-adapter`
3. `unisql-dialect-postgres`, `unisql-dialect-sqlite`, `unisql-dialect-mysql`
4. `unisql`
5. `unisql-embed`, then `unisql-macro-core` (needs `unisql`), then the later `unisql-macros`. Not published yet; their 0.0.0 placeholders are claimed.

Before each: `cargo publish --dry-run -p <crate>` (it needs its dependencies already on crates.io, so
only step 1 can be dry-run before anything is published). After step 4: `cargo add unisql` in a scratch
project and run the README example.

Gates that must be green on the release commit (they are the six required CI contexts): build, test,
clippy, fmt on ubuntu and windows; MSRV; `cargo deny`; the SPDX gate; the differential suites against
the digest-pinned engines. Run locally the same set by exit code:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check advisories licenses bans sources
bash .github/spdx_gate.sh
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p unisql -p unisql-core -p unisql-sqlparser-adapter \
  -p unisql-dialect-postgres -p unisql-dialect-sqlite -p unisql-dialect-mysql
```

## 5. docs.rs

`cargo doc --no-deps` with `-D warnings` is clean for all six crates at 0.16.0. The crates have no
features, no build scripts and no network access, so the docs.rs build needs no `[package.metadata.docs.rs]`.
The facade's crate documentation carries a doc-test; the README example is not compiled (it is the
same text): re-check it by hand on release.

## 6. Provenance and licence

- The project is `MIT OR Apache-2.0`; every tracked source file declares it (SPDX gate, floor 20 files).
- `docs/LICENCES.md` records the licences read for SQLGlot (MIT) and jOOQ (Apache-2.0 plus commercial):
  **nothing is derived from either**; a mapping derived later needs its origin file and SHA and the MIT
  notice kept (and a `THIRD-PARTY-NOTICES` file in the package). Re-read the licence at the SHA used.
- `cargo deny` licences check passes for the dependency tree (`sqlparser` is Apache-2.0).
- The logo (`assets/Unicycle.svg`) is Openclipart "Unicycle" by GDJ, public domain; the PNG copies
  are unverified and not part of any package.

## 7. Open points

- **Branch protection** requires the six CI contexts.
- **Consumers**: PersistAnt and other projects cannot depend on `unisql` until it is published (the
  crates.io-only rule); `0.x` releases are breaking by their second number, so they should pin
  `=0.16` style or a tilde requirement.
- **Yank policy**: a release with a wrong mapping is yanked and replaced; the CHANGELOG states the
  gate-verdict flips of each release, which is what a consumer needs to decide.
- **Which version is the first real release** (0.16.x is the current line; the placeholders are 0.0.0).
