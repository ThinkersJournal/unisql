// SPDX-License-Identifier: MIT OR Apache-2.0
//! Compile-fail and compile-pass cases for `sql!`. Every fail case has a passing twin in `ui/pass_*.rs`
//! (the same call with the fault removed). The `.stderr` files are rustc's output for the current stable
//! toolchain; regenerate with `TRYBUILD=overwrite` and read the diff before committing.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui/pass_*.rs");
    t.compile_fail("tests/ui/fail_*.rs");
}
