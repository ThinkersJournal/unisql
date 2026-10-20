const Q: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [mysql],
    "SELECT a FROM t WHERE name < $1"
);

fn main() {
    let _ = Q;
}
