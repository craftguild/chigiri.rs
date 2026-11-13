//! Exercise Mock subscriptions through the actual CLI listener and wire protocols.

mod assertions;
mod consumers;
mod http;
mod isolation;
mod logs;
mod pressure;
mod project;
mod sequences;
mod websocket;

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use assertions::{Goal, Profile, StreamReport};
use serde_json::json;

use crate::support::{Directory, Server};

const SUBSCRIPTION: &str =
    r#"subscription { changed(topic: "events") { id value related { label } explode } }"#;
const SEED_BASELINE_COUNT: u64 = 1;

#[test]
fn mock_subscriptions_cross_all_three_real_wire_protocols_and_recover() {
    let _test_case = crate::support::test_case();
    let directory = project::checked();
    let mut server = Server::project(directory);
    assert_seed(&server, 7, "seed-baseline");
    isolation::exercise(&server);
    let failure = wave(&server, Goal::Failure, 0);
    for report in failure {
        report.assert_contract(Goal::Failure);
    }
    assert!(
        server.graphql("{ count }")["data"]["count"]
            .as_u64()
            .unwrap()
            >= 4
    );
    pressure::exercise(&server);
    let recovery = wave(&server, Goal::Recovery, 1);
    for report in recovery {
        report.assert_contract(Goal::Recovery);
    }
    assert!(
        server.graphql("{ count }")["data"]["count"]
            .as_u64()
            .unwrap()
            >= 5
    );
    mutate_seed(&server, 901);
    assert_seed(&server, 901, "seed-mutated");
    let project = Directory(server.directory.0.clone());
    let live_http = [
        http::hold(server.address, Profile::ServerSentEvents, SUBSCRIPTION),
        http::hold(server.address, Profile::Multipart, SUBSCRIPTION),
    ];
    let live_websocket = websocket::hold(server.address, SUBSCRIPTION);
    publish(&server, "shutdown-active", 15);
    let live_websocket = live_websocket.assert_active("shutdown-active", Some(15));
    server.stop("-TERM");
    for response in live_http {
        response.assert_closed();
    }
    live_websocket.assert_closed();

    let mut restarted = Server::project(project);
    assert_eq!(
        restarted.graphql("{ count }")["data"]["count"],
        SEED_BASELINE_COUNT
    );
    assert_seed(&restarted, 7, "seed-baseline");
    for report in wave(&restarted, Goal::Recovery, 2) {
        report.assert_contract(Goal::Recovery);
    }
    restarted.stop("-TERM");
}

fn wave(server: &Server, goal: Goal, wave: usize) -> Vec<StreamReport> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (active_tx, active_rx) = mpsc::channel();
    let mut clients = Vec::new();
    for profile in [
        Profile::ServerSentEvents,
        Profile::Multipart,
        Profile::WebSocket,
    ] {
        let ready = ready_tx.clone();
        let active = active_tx.clone();
        let address = server.address;
        clients.push(thread::spawn(move || match profile {
            Profile::ServerSentEvents | Profile::Multipart => {
                http::observe(address, profile, goal, SUBSCRIPTION, ready, active)
            }
            Profile::WebSocket => websocket::observe(address, goal, SUBSCRIPTION, ready, active),
        }));
    }
    drop(ready_tx);
    drop(active_tx);
    for _ in 0..3 {
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("each real protocol must start");
    }
    activate(server, wave, active_rx);
    match goal {
        Goal::Failure => {
            assert_rollback(server);
            let pair = server.operation(json!({
                "query":"mutation($first:ID!,$second:ID!){publishPair(first:$first,second:$second)}",
                "variables":{"first":"first","second":"second"}
            }));
            assert_eq!(pair["data"]["publishPair"], 2);
            publish(server, "failure", 13);
        }
        Goal::Recovery => publish(server, "recovery", 14),
    }
    clients
        .into_iter()
        .map(|client| client.join().expect("bounded wire client"))
        .collect()
}

fn assert_rollback(server: &Server) {
    let before = server.graphql("{ count }")["data"]["count"].clone();
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"mutation{rollback(id:\"rollback\")}"}).to_string(),
    );
    assert_eq!(response.status, 500);
    let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["message"], "Mock execution failed");
    assert_eq!(
        body["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert!(!response.body.contains("private rollback failure"));
    assert_eq!(server.graphql("{ count }")["data"]["count"], before);
}

fn activate(server: &Server, wave: usize, active_rx: mpsc::Receiver<Profile>) {
    let mut active = [false; 3];
    for attempt in 0..8 {
        publish(
            server,
            &format!("warmup-{wave}-{attempt}"),
            i64::from(attempt),
        );
        while let Ok(profile) = active_rx.recv_timeout(Duration::from_millis(250)) {
            active[profile.index()] = true;
            if active.iter().all(|seen| *seen) {
                break;
            }
        }
        if active.iter().all(|seen| *seen) {
            return;
        }
    }
    panic!("all protocol sources must become active")
}

fn publish(server: &Server, id: &str, value: i64) {
    publish_to(server, "events", id, value);
}

pub(super) fn publish_to(server: &Server, topic: &str, id: &str, value: i64) {
    let result = server.operation(json!({
        "query":"mutation($topic:String!,$id:ID!,$value:Int!){publish(topic:$topic,id:$id,value:$value){id value}}",
        "variables":{"topic":topic,"id":id,"value":value}
    }));
    assert_eq!(result["data"]["publish"]["id"], id);
    assert_eq!(result["data"]["publish"]["value"], value + 100);
}

fn mutate_seed(server: &Server, value: i64) {
    let result = server.operation(json!({
        "query":"mutation($value:Int!){mutateSeed(value:$value){id value related{label}}}",
        "variables":{"value":value}
    }));
    assert_eq!(
        result["data"]["mutateSeed"],
        json!({"id":"seed","value":value,"related":{"label":"seed-mutated"}})
    );
}

fn assert_seed(server: &Server, value: i64, label: &str) {
    assert_eq!(
        server.graphql("{ seed { id value related { label } } }")["data"]["seed"],
        json!({"id":"seed","value":value,"related":{"label":label}})
    );
}
