// SPDX-License-Identifier: MIT OR Apache-2.0
const Q: unisql_embed::Query = unisql_macros::sql!(source = postgres, targets = [postgres, sqlite], "SELECT md5($1)");

fn main() {
    let _ = Q;
}
