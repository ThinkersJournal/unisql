const Q: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [sqlite, mysql],
    allow = [text_collation],
    "SELECT a FROM t WHERE x = $1 AND y = $2"
);

fn main() {
    let _ = Q;
}
