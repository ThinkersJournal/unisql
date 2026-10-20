// SPDX-License-Identifier: MIT OR Apache-2.0
const Q: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [sqlite],
    allow = [text_collation],
    requires = ["ordered_comparison"],
    forbids = ["upsert"],
    "SELECT a FROM t WHERE n < $1"
);

fn main() {
    let _ = Q;
}
