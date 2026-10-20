// SPDX-License-Identifier: MIT OR Apache-2.0
//! Every leaf id in a checked-in matrix file has a written contract, and every contract line is
//! for a leaf some matrix file names. A leaf without a contract is an id nobody can test against.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use unisql::Allowance;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn contracts() -> BTreeSet<String> {
    let text = std::fs::read_to_string(workspace_root().join("docs/LEAF-CONTRACTS.tsv")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (leaf, contract) = l.split_once('\t').expect("leaf<TAB>contract");
            assert!(!contract.trim().is_empty(), "empty contract for `{leaf}`");
            leaf.to_string()
        })
        .collect()
}

fn matrix_leaves() -> (BTreeSet<String>, usize) {
    let mut leaves = BTreeSet::new();
    let mut files = 0;
    for dir in std::fs::read_dir(workspace_root().join("crates")).unwrap() {
        let dir = dir.unwrap().path();
        for f in std::fs::read_dir(&dir).into_iter().flatten() {
            let f = f.unwrap().path();
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            if name.starts_with("matrix-") && name.ends_with(".tsv") {
                files += 1;
                for line in std::fs::read_to_string(&f).unwrap().lines().skip(1) {
                    leaves.insert(line.split('\t').nth(1).unwrap().to_string());
                }
            }
        }
    }
    (leaves, files)
}

/// `<base>.<allowance>` is covered by the base leaf's contract.
fn base_of_allowance_leaf(leaf: &str) -> Option<&str> {
    let (base, suffix) = leaf.rsplit_once('.')?;
    Allowance::ALL
        .iter()
        .any(|a| a.name() == suffix)
        .then_some(base)
}

#[test]
fn every_matrix_leaf_has_a_contract_and_every_contract_a_leaf() {
    let contracts = contracts();
    let (leaves, files) = matrix_leaves();
    assert_eq!(files, 7, "expected the seven matrix files");
    assert!(leaves.len() > 60, "only {} leaves read", leaves.len());

    let mut missing = Vec::new();
    for leaf in &leaves {
        let covered = contracts.contains(leaf)
            || base_of_allowance_leaf(leaf).is_some_and(|b| contracts.contains(b));
        if !covered {
            missing.push(leaf.clone());
        }
    }
    assert!(missing.is_empty(), "leaves with no contract: {missing:?}");

    // Leaves reached only when the caller supplies a `Schema` are exercised by schema cases, and
    // schema cases are not filed in the matrix (its `config` is `schema=none`).
    const SCHEMA_ONLY: &[&str] = &["upsert.key_from_schema"];
    let orphans: Vec<_> = contracts
        .iter()
        .filter(|c| !leaves.contains(*c) && !SCHEMA_ONLY.contains(&c.as_str()))
        .collect();
    assert!(
        orphans.is_empty(),
        "contract lines for no leaf: {orphans:?}"
    );
}

#[test]
fn the_check_can_fail() {
    // Positive control for the allowance rule and a known-bad: an invented leaf is not covered.
    let contracts = contracts();
    assert!(base_of_allowance_leaf("cast.to_text.lenient_cast").is_some());
    assert!(base_of_allowance_leaf("cast.to_text").is_none());
    assert!(!contracts.contains("cast.to_klingon"));
}

/// Each publishable crate carries its own copy of the licence texts (crates.io shows what a package
/// contains); they must be byte-identical to the workspace root's.
#[test]
fn every_published_crate_carries_the_root_licence_texts() {
    let root = workspace_root();
    let mut checked = 0;
    for dir in std::fs::read_dir(root.join("crates")).unwrap() {
        let dir = dir.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if name == "unisql-differential" {
            continue; // publish = false
        }
        for lic in ["LICENSE-MIT", "LICENSE-APACHE"] {
            let ours =
                std::fs::read(dir.join(lic)).unwrap_or_else(|e| panic!("{name} has no {lic}: {e}"));
            assert_eq!(ours, std::fs::read(root.join(lic)).unwrap(), "{name}/{lic}");
            checked += 1;
        }
        assert!(dir.join("README.md").exists(), "{name} has no README.md");
    }
    assert_eq!(checked, 18, "nine crates, two licence files each");
}
