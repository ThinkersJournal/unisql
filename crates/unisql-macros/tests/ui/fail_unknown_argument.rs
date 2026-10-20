const Q: unisql_embed::Query = unisql_macros::sql!(source = postgres, targets = [sqlite], taregts = [sqlite], "SELECT 1");

fn main() {
    let _ = Q;
}
