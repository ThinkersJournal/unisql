// SPDX-License-Identifier: MIT OR Apache-2.0
const Q: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [sqlite],
    allow = [text_collation],
    forbids = ["ordered_comparison"],
    "SELECT a FROM t WHERE n < $1"
);

fn main() {
    let _ = Q;
}
