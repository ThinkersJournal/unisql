// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let _ = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        schema = "no/such/file.sql",
        "SELECT 1"
    );
}
