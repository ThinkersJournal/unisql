// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let _ = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        allow = [text_collation],
        params = (i64),
        "SELECT a FROM t WHERE x = $1 AND y = $2"
    );
}
