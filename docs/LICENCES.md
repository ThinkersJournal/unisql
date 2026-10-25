# Third-party licence record (provenance)

Read 2026-10-12 **from the files themselves** (GitHub git-blob API, raw bytes; each
blob's `git hash-object` equals the tree entry's SHA), not from a summariser. Copies of the files read
are in `measure/licences/`. A licence file read at one SHA says nothing about another; re-read before
relying on a later release.

## SQLGlot (`tobymao/sqlglot`)

| | |
|---|---|
| Pinned SHA | `5b949bd6b7f1db1474a87ac34d3a9246fa7de237` (`main`, 2026-10-12) |
| `LICENSE` | blob `aa0703d7fc80b4b0a7a9a115de50467013c7c219`, 1065 bytes: **MIT License, "Copyright (c) 2026 Toby Mao"** |
| `pyproject.toml` | `license = "MIT"`, `license-files = ["LICENSE"]`, author Toby Mao |
| Per-file headers | none: `sqlglot/dialects/postgres.py` begins with imports (one file sampled) |
| Other licence files in the tree | `tests/fixtures/jsonpath/LICENSE`, blob `212459f6…`, 1478 bytes: **BSD-2-Clause**, for the vendored `jsonpath-compliance-test-suite` |

**Derivation permitted?** Yes, with conditions. MIT permits use, copy, modification, distribution and
sublicensing; the one condition is that *"the above copyright notice and this permission notice shall
be included in all copies or substantial portions of the Software."* So:

1. Anything of SQLGlot's that is translated or adapted into this project (a dialect table, a function
   map, an algorithm expressed as Rust, a test case) is a "substantial portion" question; the safe rule
   is **record the origin file and keep the MIT notice with it**, in the file header and in a
   `THIRD-PARTY-NOTICES` file shipped with the crate.
2. Reading SQLGlot to learn *how* a dialect behaves and then writing our own table is not copying
   protected expression; but where we mirror its structure or data closely, treat it as derived and
   credit it. "Python as pseudocode" is derivation if the output follows the source line by line.
3. **Do not copy from `tests/fixtures/jsonpath/`**: it is a separate BSD-2 project's data; use needs
   its own BSD-2 notice and its own origin trace (to `jsonpath-compliance-test-suite`).
4. The project's own licence stays `MIT OR Apache-2.0`; derived portions additionally carry the MIT
   notice naming Toby Mao. MIT is compatible with both.

**Not established:** whether SQLGlot's test fixtures (`tests/`) contain material copied from other
projects beyond the jsonpath suite (only licence-named files were searched; the tree listing was not
truncated). Check a fixture's origin before reusing any single file.

## jOOQ (`jOOQ/jOOQ`)

| | |
|---|---|
| Pinned SHA | `e450fb50aad6ed00a745b39ce74fbaff42933690` (`main`, 2026-10-12) |
| `LICENSE` | blob `f760207dfa6280bbcecda6b369f0a14bf4cfc8c4`, 872 bytes: **Apache License 2.0 by notice** ("Licensed under the Apache License, Version 2.0 ... https://www.apache.org/licenses/LICENSE-2.0"), then **"Other licenses: Commercial licenses for this work are available. These replace the above Apache-2.0 license ..."** |
| Form of the file | An abbreviated notice, not the full Apache-2.0 text (the reason GitHub reports `NOASSERTION`). The same text heads the source: `jOOQ/src/main/java/org/jooq/SQLDialect.java` (one file sampled). |
| `jOOQ/NOTICE.txt` | blob `698edf47…`, 385 bytes: third-party notice for Apache Commons Lang (ASF) |
| Tree | 67 `LICENSE.txt`/`NOTICE.txt` files, all one of the two blobs above; tree not truncated |

**Derivation permitted?** For the open-source repository content: yes, under Apache-2.0, **with
conditions** (retain copyright/licence/notice, state changes, carry applicable `NOTICE` text; Apache-2.0
also grants a patent licence with the usual termination on patent suit). **Caveat:** the file offers a
separate commercial licence "for commercial database integrations"; this record does not establish
which databases' support lives in the open repo versus commercial editions, only that the repo's
files are offered under Apache-2.0. Do not assume code for a commercial-database dialect is present or
licensed here. As the goal is to glean concepts from a Java codebase, the practical rule is: **ideas
(the `@Support` annotation model, emulation catalogue) are free; code is Apache-2.0 with notice.**

## Our own dependency, for completeness

`sqlparser` 0.63.0: Apache-2.0 (crates.io API, 2026-10-12; DESIGN.md section 4). Byte-read 2026-10-25:
the crate as `cargo` unpacks it (`sqlparser-0.63.0/LICENSE.TXT`, 10847 bytes, sha256
`a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2`, git blob `16fe87b06e802f094b3fbb0894b137bca2b16ef1`)
is byte-identical to `LICENSE.TXT` at upstream tag `v0.63.0` (commit `85b1a6f2223bf95d98cf1bc504971abd191dc869`,
same blob SHA and sha256, from `apache/datafusion-sqlparser-rs`): the Apache License 2.0 text. The tag's root has no
`NOTICE` file, so there is no NOTICE to carry. We link it unmodified. Re-read when the pin moves.

## Summary

| Source | Licence | May we derive? | Obligation |
|---|---|---|---|
| SQLGlot | MIT | yes | keep MIT notice (Toby Mao) with derived portions; separate BSD-2 for `tests/fixtures/jsonpath` |
| jOOQ | Apache-2.0 (commercial alternative for commercial integrations) | yes, open repo content | keep notices, state changes, carry NOTICE; verify which dialects are in the open repo |

## The logo (`assets/Unicycle*.png`, `assets/Unicycle.svg`)

Source given by the project owner 2026-10-12: https://openclipart.org/detail/221146/unicycle

| | |
|---|---|
| Page read | 2026-10-12, saved at `measure/licences/openclipart-221146.html` (12,901 bytes) |
| Title / artist | "Unicycle" by **GDJ**, uploaded June 21, 2015 (page text) |
| Licence statement | The site footer states **"Openclipart is 100% Public Domain"** (page text); the licence link is the site's `/share` page, which was not separately read |
| File check | `assets/Unicycle.svg` is **byte-identical** to `https://openclipart.org/download/221146/Unicycle.svg` (14,157 bytes, same SHA-256; the download is saved in `measure/licences/`) |
| Not verified | The three PNGs were not byte-compared to anything; they look like renders of the SVG at 807x1406, 323x563 and 162x283. Their licence follows the SVG only if they are derived from it |

Public-domain dedication needs no notice, but credit costs nothing: **Unicycle, by GDJ, Openclipart.**
Whether the dedication on the site is valid for this specific upload is the site's claim, not
something this record can establish.
