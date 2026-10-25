// SPDX-License-Identifier: MIT OR Apache-2.0
fn main() {
    let (_q, (_a, _b)) = unisql_macros::sql!(
        source = postgres,
        targets = [sqlite],
        allow = [text_collation],
        params = (i64, Option<String>),
        "SELECT a FROM t WHERE x = $1 AND y = $2",
        7_i64,
        None::<String>
    );
}
