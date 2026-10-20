const Q: unisql_embed::Query = unisql_macros::sql!(source = postgres, targets = [sqlite]);

fn main() {
    let _ = Q;
}
