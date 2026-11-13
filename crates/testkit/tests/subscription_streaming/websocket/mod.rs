//! The two live WebSocket consumers complement the retained four HTTP consumers.

mod cancellation;
mod client;
mod errors;
mod protocol;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use serde_json::Value;

use super::fixture;
use super::server::{Adapter, RunningServer};
use super::validation::assert_default_delivery;
use client::Client;

const TICKS: &str = "subscription { ticks { value } }";
const FAILING: &str = "subscription { failingTicks { value } }";
const GREETING: &str = "query { greeting }";
const MUTATION: &str = "mutation { acknowledge }";

pub(super) fn run(adapter: Adapter) {
    ordered_multiplexed_operations(adapter);
    errors::run(adapter);
    cancellation::run(adapter);
    protocol::run(adapter);
    unread_slow_consumer(adapter);
}

fn ordered_multiplexed_operations(adapter: Adapter) {
    let (endpoint, mapped) = fixture::endpoint();
    let server = RunningServer::start_websocket(adapter, endpoint);
    let mut client = Client::connect(server.address());
    client.initialize();
    client.subscribe("subscription", TICKS);
    client.subscribe("query", GREETING);
    client.subscribe("mutation", MUTATION);
    let messages = through_terminals(&mut client, &["subscription", "query", "mutation"]);
    let grouped = group(messages);
    for (index, expected) in [1, 2].into_iter().enumerate() {
        assert_eq!(grouped["subscription"][index]["type"], "next");
        assert_eq!(
            grouped["subscription"][index]["payload"]["data"]["ticks"]["value"],
            expected
        );
        assert_default_delivery(&grouped["subscription"][index]["payload"]);
    }
    assert_eq!(grouped["subscription"].len(), 3);
    assert_eq!(grouped["query"][0]["payload"]["data"]["greeting"], "hello");
    assert_eq!(
        grouped["mutation"][0]["payload"]["data"]["acknowledge"],
        "accepted"
    );
    for id in ["query", "mutation"] {
        assert_eq!(grouped[id].len(), 2);
        assert_eq!(grouped[id][1]["type"], "complete");
    }
    client.close();
    drop(client);
    server.finish();
    assert_eq!(mapped.load(Ordering::SeqCst), 0);
}

fn through_terminals(client: &mut Client, ids: &[&str]) -> Vec<Value> {
    let mut pending: BTreeSet<_> = ids.iter().copied().collect();
    let mut messages = Vec::new();
    for _ in 0..64 {
        let message = client.next();
        if matches!(message["type"].as_str(), Some("complete" | "error")) {
            let id = message["id"].as_str().expect("operation terminal ID");
            assert!(
                pending.remove(id),
                "duplicate or unknown operation terminal: {message}"
            );
        }
        messages.push(message);
        if pending.is_empty() {
            return messages;
        }
    }
    panic!("finite operation frame bound");
}

fn unread_slow_consumer(adapter: Adapter) {
    let (endpoint, mapped, probe) = fixture::endpoint_with_flood();
    let server = RunningServer::start_websocket(adapter, endpoint);
    let mut client = Client::connect(server.address());
    client.initialize();
    client.subscribe("unread", "subscription { ticks { value padding } }");
    probe.wait_until_pressure_cancels();
    drop(client);
    server.finish();
    assert_eq!(mapped.load(Ordering::SeqCst), 0);
}

fn group(messages: Vec<Value>) -> BTreeMap<String, Vec<Value>> {
    let mut grouped = BTreeMap::<_, Vec<_>>::new();
    for message in messages {
        let id = message["id"]
            .as_str()
            .expect("operation message ID")
            .to_owned();
        grouped.entry(id).or_default().push(message);
    }
    grouped
}
