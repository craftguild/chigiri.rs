//! Verifies that the UI surface remains unavailable without its feature.

fn publication(endpoint: chigiri::Endpoint<()>) {
    let _ = endpoint.graphiql();
}

fn main() {
    let _ = publication as fn(chigiri::Endpoint<()>);
}
