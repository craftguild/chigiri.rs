//! Correlate private lifecycle JSONL with three actual subscription wire profiles.

use std::collections::BTreeSet;
use std::fs;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::assertions::Profile;
use super::{SEED_BASELINE_COUNT, SUBSCRIPTION, http, project, websocket};
use crate::support::Server;
use contract::{SetupIdentity, assert_final_contract, setup_identity};

mod contract;

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn real_wire_lifecycles_have_exact_private_log_v5_correlation() {
    let _test_case = crate::support::test_case();
    let mut server = Server::project(project::checked());
    let mut known = BTreeSet::new();

    let sse = http::hold(server.address, Profile::ServerSentEvents, SUBSCRIPTION);
    let sse_setup = await_new_setup(&server, &known);
    known.insert(sse_setup.generation);
    let multipart = http::hold(server.address, Profile::Multipart, SUBSCRIPTION);
    let multipart_setup = await_new_setup(&server, &known);
    known.insert(multipart_setup.generation);
    let websocket = websocket::hold(server.address, SUBSCRIPTION);
    let websocket_setup = await_new_setup(&server, &known);
    known.insert(websocket_setup.generation);

    assert_eq!(
        server.graphql("{ count }")["data"]["count"],
        SEED_BASELINE_COUNT
    );
    let rollback_request = rollback_request(&server);
    assert_eq!(
        server.graphql("{ count }")["data"]["count"],
        SEED_BASELINE_COUNT
    );
    super::publish(&server, "logged-event", 21);
    assert_eq!(
        server.graphql("{ count }")["data"]["count"],
        SEED_BASELINE_COUNT + 1
    );

    let sse = sse.assert_active("logged-event");
    let multipart = multipart.assert_active("logged-event");
    let websocket = websocket.assert_active("logged-event", Some(21));

    // Five finite requests and the upgrade finish independently of their
    // native response waiters. Observe those access identities before closing
    // either HTTP stream; unrelated records may interleave with terminals.
    let finite_access = await_accesses(&server, 6);

    websocket.complete();
    await_terminal(&server, websocket_setup.generation, "client_cancelled");
    drop(sse);
    await_terminal(&server, sse_setup.generation, "transport_closed");
    let after_sse = await_accesses(&server, 7);
    let sse_access: Vec<_> = after_sse.difference(&finite_access).copied().collect();
    let [sse_access] = sse_access.as_slice() else {
        panic!("one native access identity follows the sole disconnected HTTP stream")
    };

    server.stop("-TERM");
    multipart.assert_closed();
    let records = final_records(&server);
    let all_access = access_ids(&records);
    assert_eq!(all_access.len(), 8);
    let multipart_access: Vec<_> = all_access.difference(&after_sse).copied().collect();
    let [multipart_access] = multipart_access.as_slice() else {
        panic!("one native access identity follows the sole remaining HTTP stream")
    };
    assert_final_contract(
        &records,
        [sse_setup, multipart_setup, websocket_setup],
        rollback_request,
        [*sse_access, *multipart_access],
    );
}

fn rollback_request(server: &Server) -> u64 {
    let response = server.http(
        "POST",
        &[("Content-Type", "application/json")],
        &json!({"query":"mutation{rollback(id:\"rollback\")}"}).to_string(),
    );
    assert_eq!(response.status, 500);
    let body: Value = serde_json::from_str(&response.body).unwrap();
    assert_eq!(body["errors"][0]["message"], "Mock execution failed");
    assert_eq!(
        body["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    body["errors"][0]["extensions"]["requestId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

fn await_new_setup(server: &Server, known: &BTreeSet<u64>) -> SetupIdentity {
    let deadline = Instant::now() + WAIT;
    loop {
        let fresh: Vec<_> = live_records(server)
            .into_iter()
            .filter(|record| record["kind"] == "subscription_setup")
            .filter_map(|record| setup_identity(&record))
            .filter(|identity| !known.contains(&identity.generation))
            .collect();
        match fresh.as_slice() {
            [identity] => return *identity,
            [] if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            [] => panic!("registered setup log did not arrive before its original deadline"),
            _ => panic!("one sequential wire start produced multiple setup generations"),
        }
    }
}

fn await_terminal(server: &Server, generation: u64, cause: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        let terminals: Vec<_> = live_records(server)
            .into_iter()
            .filter(|record| {
                record["kind"] == "subscription_terminal" && record["subscription_id"] == generation
            })
            .collect();
        match terminals.as_slice() {
            [terminal] => {
                assert_eq!(terminal["cause"], cause);
                return;
            }
            [] if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            [] => panic!("logical terminal log did not arrive before its original deadline"),
            _ => panic!("one generation emitted more than one terminal record"),
        }
    }
}

fn live_records(server: &Server) -> Vec<Value> {
    let bytes = fs::read(server.directory.0.join("stdout.log")).unwrap();
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    parse(&bytes[..complete])
}

fn access_ids(records: &[Value]) -> BTreeSet<u64> {
    let access: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "access")
        .map(|record| record["request_id"].as_u64().unwrap())
        .collect();
    let identities: BTreeSet<_> = access.iter().copied().collect();
    assert_eq!(identities.len(), access.len(), "one access per request");
    identities
}

fn await_accesses(server: &Server, count: usize) -> BTreeSet<u64> {
    let deadline = Instant::now() + WAIT;
    loop {
        let identities = access_ids(&live_records(server));
        assert!(identities.len() <= count, "unexpected native access record");
        if identities.len() == count {
            return identities;
        }
        assert!(
            Instant::now() < deadline,
            "native access records did not arrive before the original deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn final_records(server: &Server) -> Vec<Value> {
    let bytes = fs::read(server.directory.0.join("stdout.log")).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    parse(&bytes)
}

fn parse(bytes: &[u8]) -> Vec<Value> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("stdout is exclusive JSONL"))
        .collect()
}
