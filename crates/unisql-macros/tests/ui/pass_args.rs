// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let (q, (a, b)) = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        allow = [text_collation],
        "SELECT a FROM t WHERE x = $1 AND y = $2",
        7_i64,
        "name"
    );
    let _: (i64, &str) = (a, b);
    let _ = q;
}
