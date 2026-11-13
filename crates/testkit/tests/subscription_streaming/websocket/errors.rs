//! Wire errors are operation-local and never followed by successful completion.

use std::sync::atomic::Ordering;

use super::super::{
    fixture,
    server::{Adapter, RunningServer},
};
use super::{Client, FAILING, GREETING, TICKS, assert_default_delivery, group, through_terminals};

pub(super) fn run(adapter: Adapter) {
    source_failure_preserves_peers(adapter);
    for (document, code, initializer) in [
        ("subscription {", "GRAPHQL_PARSE_FAILED", false),
        (
            "subscription { missing { value } }",
            "GRAPHQL_VALIDATION_FAILED",
            false,
        ),
        (FAILING, "INITIALIZER_DENIED", true),
    ] {
        let (endpoint, mapped) = if initializer {
            fixture::endpoint_with_initializer_error()
        } else {
            fixture::endpoint()
        };
        let server = RunningServer::start_websocket(adapter, endpoint);
        let mut client = Client::connect(server.address());
        client.initialize();
        client.subscribe("failed", document);
        let error = client.next();
        assert_eq!(error["type"], "error");
        assert_eq!(error["id"], "failed");
        assert_eq!(error["payload"][0]["extensions"]["code"], code);
        client.subscribe("barrier", GREETING);
        let barrier = group(through_terminals(&mut client, &["barrier"]));
        assert_eq!(
            barrier.len(),
            1,
            "no later complete for the failed operation"
        );
        client.close();
        drop(client);
        server.finish();
        assert_eq!(mapped.load(Ordering::SeqCst), usize::from(initializer));
    }
}

fn source_failure_preserves_peers(adapter: Adapter) {
    let (endpoint, mapped) = fixture::endpoint();
    let server = RunningServer::start_websocket(adapter, endpoint);
    let mut client = Client::connect(server.address());
    client.initialize();
    client.subscribe("failed", FAILING);
    client.subscribe("healthy", TICKS);
    client.subscribe("query", GREETING);
    let grouped = group(through_terminals(
        &mut client,
        &["failed", "healthy", "query"],
    ));
    assert_eq!(grouped["failed"].len(), 2);
    assert_eq!(
        grouped["failed"][0]["payload"]["data"]["failingTicks"]["value"],
        7
    );
    assert_default_delivery(&grouped["failed"][0]["payload"]);
    assert_eq!(grouped["failed"][1]["type"], "error");
    assert_eq!(
        grouped["failed"][1]["payload"][0]["extensions"]["code"],
        "SOURCE_FAILED"
    );
    assert_eq!(grouped["healthy"].len(), 3);
    client.subscribe("barrier", GREETING);
    let barrier = group(through_terminals(&mut client, &["barrier"]));
    assert_eq!(barrier.len(), 1);
    client.close();
    drop(client);
    server.finish();
    assert_eq!(mapped.load(Ordering::SeqCst), 1);
}
