// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let _ = unisql_macros::sql!(source = postgres, targets = [sqlite], "SELECT 1", 7_i64);
}
