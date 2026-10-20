const Q: unisql_embed::Query = unisql_macros::sql!(
    source = postgres,
    targets = [mysql],
    allow = [text_collatoin],
    "SELECT 1"
);

fn main() {
    let _ = Q;
}
