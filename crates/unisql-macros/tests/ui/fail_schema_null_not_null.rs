// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let _ = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        schema_ddl = "CREATE TABLE kv (k TEXT PRIMARY KEY, n BIGINT NOT NULL);",
        "INSERT INTO kv (k, n) VALUES ($1, NULL)"
    );
}
