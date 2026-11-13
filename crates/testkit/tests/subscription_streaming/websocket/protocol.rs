//! Real opening defenses, initialization authority and operation-ID boundaries.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri::{
    BoxFuture, ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision,
    ConnectionInitError,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use super::super::{
    fixture,
    server::{Adapter, RunningServer},
};
use super::{Client, GREETING, TICKS, group, through_terminals};

pub(super) fn run(adapter: Adapter) {
    for (protocol, preflight, status) in [
        (None, true, 400),
        (Some("graphql-ws"), true, 400),
        (Some("graphql-transport-ws"), false, 403),
    ] {
        let (endpoint, _) = fixture::endpoint();
        let server = RunningServer::start_websocket(adapter, endpoint);
        Client::assert_rejected(server.address(), protocol, preflight, status);
        server.finish();
    }
    for (message, code) in [
        (json!({"type":"connection_init", "payload":[]}), 4400),
        (json!({"type":"unknown"}), 4400),
        (
            json!({"id":"early","type":"subscribe","payload":{"query":GREETING}}),
            4401,
        ),
        (
            json!({"type":"connection_init","payload":{"token":"x".repeat(16 * 1024)}}),
            4400,
        ),
        (
            json!({"type":"ping","payload":{"large":"x".repeat(16 * 1024)}}),
            4400,
        ),
    ] {
        let (endpoint, mapped) = fixture::endpoint();
        let server = RunningServer::start_websocket(adapter, endpoint);
        let mut client = Client::connect(server.address());
        client.send(message);
        client.expect_close(code);
        drop(client);
        server.finish();
        assert_eq!(mapped.load(Ordering::SeqCst), 0);
    }
    for frame in [Message::Text("{".into()), Message::Binary(vec![0].into())] {
        let (endpoint, _) = fixture::endpoint();
        let server = RunningServer::start_websocket(adapter, endpoint);
        let mut client = Client::connect(server.address());
        client.frame(frame);
        client.expect_close(4400);
        drop(client);
        server.finish();
    }
    repeated_initialization(adapter);
    oversized_id_preserves_peer(adapter);
    duplicate_live_id(adapter);
    authorization(adapter);
}

fn repeated_initialization(adapter: Adapter) {
    let (endpoint, _) = fixture::endpoint();
    let server = RunningServer::start_websocket(adapter, endpoint);
    let mut client = Client::connect(server.address());
    client.initialize();
    client.send(json!({"type":"connection_init"}));
    client.expect_close(4429);
    drop(client);
    server.finish();
}

fn oversized_id_preserves_peer(adapter: Adapter) {
    let (endpoint, mapped) = fixture::endpoint();
    let server = RunningServer::start_websocket(adapter, endpoint);
    let mut client = Client::connect(server.address());
    client.initialize();
    let id = "x".repeat(257);
    client.subscribe(&id, GREETING);
    client.subscribe("peer", GREETING);
    let grouped = group(through_terminals(&mut client, &[&id, "peer"]));
    assert_eq!(grouped[&id].len(), 1);
    assert_eq!(grouped[&id][0]["type"], "error");
    assert_eq!(grouped["peer"].len(), 2);
    assert_eq!(grouped["peer"][0]["payload"]["data"]["greeting"], "hello");
    client.close();
    drop(client);
    server.finish();
    assert_eq!(mapped.load(Ordering::SeqCst), 0);
}

fn duplicate_live_id(adapter: Adapter) {
    let (endpoint, _, probe) = fixture::endpoint_with_pending_ticks();
    let server = RunningServer::start_websocket(adapter, endpoint);
    let mut client = Client::connect(server.address());
    client.initialize();
    client.subscribe("duplicate", TICKS);
    for expected in [1, 2] {
        assert_eq!(client.next()["payload"]["data"]["ticks"]["value"], expected);
    }
    probe.wait_until_pending();
    client.subscribe("duplicate", GREETING);
    client.expect_close(4409);
    drop(client);
    probe.wait_until_cancelled();
    server.finish();
    probe.assert_cancelled();
}

struct Authorizer {
    calls: Arc<AtomicUsize>,
    internal_failure: bool,
}

impl ConnectionInitAuthorizer for Authorizer {
    fn authorize<'a>(
        &'a self,
        request: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        assert_eq!(
            request.payload().unwrap().get("token"),
            Some(&chigiri::Value::String("private-token".to_owned()))
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.internal_failure {
                Err(ConnectionInitError::new(std::io::Error::other(
                    "private failure",
                )))
            } else {
                Ok(ConnectionInitDecision::Reject)
            }
        })
    }
}

fn authorization(adapter: Adapter) {
    for internal_failure in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (endpoint, mapped) = fixture::endpoint_with_authorizer(Authorizer {
            calls: Arc::clone(&calls),
            internal_failure,
        });
        let server = RunningServer::start_websocket(adapter, endpoint);
        let mut client = Client::connect(server.address());
        client.send(json!({"type":"connection_init","payload":{"token":"private-token"}}));
        client.expect_close(if internal_failure { 1011 } else { 4403 });
        drop(client);
        server.finish();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(mapped.load(Ordering::SeqCst), 0);
    }
}
