// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let _ = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        schema = "tests/schema.sql",
        schema_ddl = "CREATE TABLE kv (k TEXT);",
        "SELECT 1"
    );
}
