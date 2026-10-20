// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let _ = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        schema_ddl = "CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT);",
        "INSERT INTO kv VALUES ($1)"
    );
}
