//! Exact private lifecycle record shapes and cross-record identity relations.

use std::collections::BTreeSet;

use serde_json::Value;

const COORDINATE: &str = "Subscription.changed";

#[derive(Clone, Copy)]
pub(super) struct SetupIdentity {
    pub(super) request: u64,
    pub(super) generation: u64,
}

pub(super) fn assert_final_contract(
    records: &[Value],
    setups: [SetupIdentity; 3],
    rollback_request: u64,
    stream_access: [u64; 2],
) {
    let subscription: Vec<_> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record["kind"].as_str().is_some_and(|kind| {
                matches!(
                    kind,
                    "subscription_setup"
                        | "subscription_publication"
                        | "subscription_drop"
                        | "subscription_terminal"
                )
            })
        })
        .collect();
    assert_eq!(subscription.len(), 7);
    assert_eq!(kind_count(&subscription, "subscription_setup"), 3);
    assert_eq!(kind_count(&subscription, "subscription_publication"), 1);
    assert_eq!(kind_count(&subscription, "subscription_drop"), 0);
    assert_eq!(kind_count(&subscription, "subscription_terminal"), 3);

    for (_, record) in &subscription {
        assert_envelope(record);
        assert_eq!(record["coordinate"], COORDINATE);
    }
    for setup in setups {
        assert_setup_and_terminal(records, setup);
        assert!(
            records.iter().all(|record| {
                record["kind"] != "access" || record["request_id"] != setup.request
            })
        );
    }
    assert_publication(records, &subscription, rollback_request);
    assert_upgrade_access(records, setups[2]);
    assert_stream_access(records, setups[0], stream_access[0], "cancelled");
    assert_stream_access(records, setups[1], stream_access[1], "completed");
    assert_terminal(
        records,
        setups[0].generation,
        "cancelled",
        "transport_closed",
    );
    assert_terminal(
        records,
        setups[1].generation,
        "cancelled",
        "server_shutdown",
    );
    assert_terminal(
        records,
        setups[2].generation,
        "cancelled",
        "client_cancelled",
    );

    let encoded = subscription
        .iter()
        .map(|(_, record)| record.to_string())
        .collect::<String>();
    for private in [
        "events",
        "logged-event",
        "rollback",
        "private rollback failure",
        "payload",
        "variables",
    ] {
        assert!(!encoded.contains(private), "private marker {private:?}");
    }
}

fn assert_publication(records: &[Value], subscription: &[(usize, &Value)], rollback_request: u64) {
    let publication = subscription
        .iter()
        .find(|(_, record)| record["kind"] == "subscription_publication")
        .unwrap()
        .1;
    assert_keys(
        publication,
        &[
            "format",
            "timestamp",
            "request_id",
            "kind",
            "sequence",
            "index",
            "coordinate",
            "outcome",
        ],
    );
    let publication_request = publication["request_id"].as_u64().unwrap();
    assert_ne!(publication_request, rollback_request);
    assert!(
        publication["sequence"]
            .as_u64()
            .is_some_and(|value| value > 0)
    );
    assert_eq!(publication["index"], 0);
    assert_eq!(publication["outcome"], "staged");
    assert_access_for_request(records, publication_request, 200);
    assert_access_for_request(records, rollback_request, 500);
    assert!(records.iter().all(|record| {
        record["kind"] != "subscription_publication" || record["request_id"] != rollback_request
    }));
}

fn assert_setup_and_terminal(records: &[Value], setup: SetupIdentity) {
    let setup_records: Vec<_> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record["kind"] == "subscription_setup" && record["subscription_id"] == setup.generation
        })
        .collect();
    let terminal_records: Vec<_> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record["kind"] == "subscription_terminal"
                && record["subscription_id"] == setup.generation
        })
        .collect();
    let [(setup_index, setup_record)] = setup_records.as_slice() else {
        panic!("one registered setup per generation")
    };
    let [(terminal_index, _)] = terminal_records.as_slice() else {
        panic!("one terminal per generation")
    };
    assert!(setup_index < terminal_index);
    assert_keys(
        setup_record,
        &[
            "format",
            "timestamp",
            "request_id",
            "kind",
            "subscription_id",
            "coordinate",
            "overflow",
            "outcome",
        ],
    );
    assert_eq!(setup_record["request_id"], setup.request);
    assert_eq!(setup_record["overflow"], "backpressure");
    assert_eq!(setup_record["outcome"], "registered");
}

fn assert_upgrade_access(records: &[Value], setup: SetupIdentity) {
    let upgrades: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "access" && record["status"] == 101)
        .collect();
    let [upgrade] = upgrades.as_slice() else {
        panic!("one actual WebSocket upgrade access record")
    };
    assert_access_shape(upgrade, "GET", 101, "completed");
    assert_ne!(upgrade["request_id"], setup.request);
}

#[test]
fn upgrade_access_correlation_accepts_both_fair_writer_orders() {
    let setup = SetupIdentity {
        request: 2,
        generation: 1,
    };
    let upgrade = serde_json::json!({
        "format": "chigiri-mock-log/v5", "timestamp": 1, "request_id": 1,
        "kind": "access", "method": "GET", "path": "/graphql", "status": 101,
        "duration_ms": 0.0, "outcome": "completed"
    });
    let registered = serde_json::json!({
        "format": "chigiri-mock-log/v5", "timestamp": 2, "request_id": 2,
        "kind": "subscription_setup", "subscription_id": 1,
        "coordinate": "Subscription.changed", "overflow": "backpressure",
        "outcome": "registered"
    });
    // Native upgrade precedes setup, but separate fair output queues can
    // publish their records in either order while preserving per-class FIFO.
    for records in [
        vec![upgrade.clone(), registered.clone()],
        vec![registered, upgrade],
    ] {
        assert_upgrade_access(&records, setup);
    }
}

fn assert_stream_access(records: &[Value], setup: SetupIdentity, request: u64, outcome: &str) {
    // ADR-0054 reserves separate fair queues for access and subscription logs.
    // Correlate native identities without imposing cross-class stdout order.
    let access: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "access" && record["request_id"] == request)
        .collect();
    let [access] = access.as_slice() else {
        panic!("one access record must correlate the observed native stream request")
    };
    assert_access_shape(access, "POST", 200, outcome);
    assert_ne!(access["request_id"], setup.request);
}

fn assert_terminal(records: &[Value], generation: u64, outcome: &str, cause: &str) {
    let terminal = records
        .iter()
        .find(|record| {
            record["kind"] == "subscription_terminal" && record["subscription_id"] == generation
        })
        .unwrap();
    assert_keys(
        terminal,
        &[
            "format",
            "timestamp",
            "request_id",
            "kind",
            "subscription_id",
            "coordinate",
            "outcome",
            "cause",
            "dropped_total",
        ],
    );
    assert!(terminal["request_id"].is_null());
    assert_eq!(terminal["outcome"], outcome);
    assert_eq!(terminal["cause"], cause);
    assert_eq!(terminal["dropped_total"], 0);
}

fn assert_access_for_request(records: &[Value], request: u64, status: u64) {
    let access: Vec<_> = records
        .iter()
        .filter(|record| record["kind"] == "access" && record["request_id"] == request)
        .collect();
    let [access] = access.as_slice() else {
        panic!("one access record must correlate request {request}")
    };
    assert_access_shape(access, "POST", status, "completed");
}

fn assert_access_shape(record: &Value, method: &str, status: u64, outcome: &str) {
    assert_envelope(record);
    assert_keys(
        record,
        &[
            "format",
            "timestamp",
            "request_id",
            "kind",
            "method",
            "path",
            "status",
            "duration_ms",
            "outcome",
        ],
    );
    assert_eq!(record["kind"], "access");
    assert_eq!(record["method"], method);
    assert_eq!(record["path"], "/graphql");
    assert_eq!(record["status"], status);
    assert!(
        record["duration_ms"]
            .as_f64()
            .is_some_and(|value| value >= 0.0)
    );
    assert_eq!(record["outcome"], outcome);
}

pub(super) fn setup_identity(record: &Value) -> Option<SetupIdentity> {
    (record["outcome"] == "registered").then(|| SetupIdentity {
        request: record["request_id"].as_u64().unwrap(),
        generation: record["subscription_id"].as_u64().unwrap(),
    })
}

fn assert_envelope(record: &Value) {
    assert_eq!(record["format"], "chigiri-mock-log/v5");
    assert!(record["timestamp"].as_i64().is_some_and(|value| value > 0));
}

fn assert_keys(record: &Value, expected: &[&str]) {
    let actual: BTreeSet<_> = record
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    assert_eq!(actual, expected);
}

fn kind_count(records: &[(usize, &Value)], kind: &str) -> usize {
    records
        .iter()
        .filter(|(_, record)| record["kind"] == kind)
        .count()
}
