//! Real CLI sequence generation, completion and restart through all three profiles.

use serde_json::{Value, json};

use super::assertions::Profile;
use super::{SUBSCRIPTION, assert_seed, http, mutate_seed, project, websocket};
use crate::support::{Directory, Server};

fn expected(value: i64, label: &str) -> Value {
    json!({"id":"seed","value":value,"related":{"label":label},"explode":value})
}

#[test]
fn finite_generators_complete_over_all_three_protocols_and_restart_from_seeds() {
    let _test_case = crate::support::test_case();
    let directory = project::checked_sequence(false);
    let mut server = Server::project(directory);
    for restart in 0..2 {
        let mut sse = http::hold(server.address, Profile::ServerSentEvents, SUBSCRIPTION);
        let mut multipart = http::hold(server.address, Profile::Multipart, SUBSCRIPTION);
        let mut websocket = websocket::hold(server.address, SUBSCRIPTION);
        assert_eq!(sse.next_changed(), expected(7, "seed-baseline"));
        assert_eq!(multipart.next_changed(), expected(7, "seed-baseline"));
        assert_eq!(websocket.next_changed(), expected(7, "seed-baseline"));
        sse.assert_complete();
        multipart.assert_complete();
        websocket.assert_complete();
        if restart == 0 {
            mutate_seed(&server, 501);
            let project = Directory(server.directory.0.clone());
            server.stop("-TERM");
            server = Server::project(project);
            assert_seed(&server, 7, "seed-baseline");
        }
    }
    server.stop("-TERM");
}

#[test]
fn repeating_generators_keep_captured_roots_with_fresh_related_reads_over_all_protocols() {
    let _test_case = crate::support::test_case();
    let mut server = Server::project(project::checked_sequence(true));
    let mut sse = http::hold(server.address, Profile::ServerSentEvents, SUBSCRIPTION);
    let mut multipart = http::hold(server.address, Profile::Multipart, SUBSCRIPTION);
    let mut websocket = websocket::hold(server.address, SUBSCRIPTION);
    assert_eq!(sse.next_changed(), expected(7, "seed-baseline"));
    assert_eq!(multipart.next_changed(), expected(7, "seed-baseline"));
    assert_eq!(websocket.next_changed(), expected(7, "seed-baseline"));
    mutate_seed(&server, 501);
    let mut fresh = http::hold(server.address, Profile::ServerSentEvents, SUBSCRIPTION);
    assert_eq!(fresh.next_changed(), expected(501, "seed-mutated"));
    observe_fresh(|| sse.next_changed());
    observe_fresh(|| multipart.next_changed());
    observe_fresh(|| websocket.next_changed());
    websocket.complete();
    drop((sse, multipart, fresh));
    server.stop("-TERM");
}

fn observe_fresh(mut next: impl FnMut() -> Value) {
    for _ in 0..128 {
        let event = next();
        assert_eq!(event["id"], "seed");
        assert_eq!(event["value"], 7);
        if event["related"]["label"] == "seed-mutated" {
            assert_eq!(event, expected(7, "seed-mutated"));
            return;
        }
    }
    panic!("fresh event snapshot did not reach the related update");
}
