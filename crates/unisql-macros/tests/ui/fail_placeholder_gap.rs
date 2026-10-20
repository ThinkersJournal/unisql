const Q: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [sqlite],
    allow = [text_collation],
    "SELECT a FROM t WHERE x = $1 AND y = $3"
);

fn main() {
    let _ = Q;
}
