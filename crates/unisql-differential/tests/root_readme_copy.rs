// SPDX-License-Identifier: MIT OR Apache-2.0
//! `ROOT-README.md` is the copy of the repository README that `src/lib.rs` compiles as doc-tests
//! (it must live inside this crate's directory). It has to stay equal to the real file.

use std::path::Path;

fn normalised(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn root_readme_copy_matches_the_root_readme() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let copy = normalised(&here.join("ROOT-README.md"));
    let root = normalised(&here.join("../../README.md"));
    assert!(
        copy == root,
        "crates/unisql-differential/ROOT-README.md differs from README.md; copy README.md over it"
    );
}
