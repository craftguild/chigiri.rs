//! Assert exact nullable lifecycle schemas, closed pairs, bounds, and privacy.

use chigiri::__private::SubscriptionTermination;
use chigiri::SubscriptionOverflow;
use serde_json::{Value, json};

use super::super::lifecycle::{self, SetupOutcome};
use crate::publication::MAX_SAFE_SEQUENCE;

fn value(bytes: Vec<u8>) -> Value {
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(bytes.iter().filter(|byte| **byte == b'\n').count(), 1);
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["timestamp"].as_i64().unwrap() > 0);
    value.as_object_mut().unwrap().remove("timestamp");
    value
}

#[test]
fn setup_has_exact_registered_failed_and_cancelled_nullable_shapes() {
    for (decision, expected) in [
        (
            SetupOutcome::Registered {
                subscription_id: 1,
                overflow: SubscriptionOverflow::Backpressure,
            },
            json!({"subscription_id":1,"overflow":"backpressure","outcome":"registered"}),
        ),
        (
            SetupOutcome::Registered {
                subscription_id: MAX_SAFE_SEQUENCE,
                overflow: SubscriptionOverflow::DropOldest,
            },
            json!({"subscription_id":MAX_SAFE_SEQUENCE,"overflow":"drop_oldest","outcome":"registered"}),
        ),
        (
            SetupOutcome::Failed,
            json!({"subscription_id":null,"overflow":null,"outcome":"failed"}),
        ),
        (
            SetupOutcome::Cancelled,
            json!({"subscription_id":null,"overflow":null,"outcome":"cancelled"}),
        ),
    ] {
        let mut expected = expected.as_object().unwrap().clone();
        expected.insert("format".into(), json!("chigiri-mock-log/v5"));
        expected.insert("request_id".into(), json!(u64::MAX));
        expected.insert("kind".into(), json!("subscription_setup"));
        expected.insert("coordinate".into(), json!("Subscription.changed"));
        assert_eq!(
            value(lifecycle::setup(u64::MAX, "Subscription.changed", decision).unwrap()),
            Value::Object(expected)
        );
    }
}

#[test]
fn drop_and_all_terminal_pairs_are_exact_and_redacted() {
    let drop = value(lifecycle::drop_oldest(1, "Subscription.changed", 1).unwrap());
    assert_eq!(
        drop,
        json!({
            "format":"chigiri-mock-log/v5","request_id":null,
            "kind":"subscription_drop","subscription_id":1,
            "coordinate":"Subscription.changed","reason":"drop_oldest","dropped_total":1
        })
    );
    for (decision, outcome, cause) in [
        (
            SubscriptionTermination::SourceComplete,
            "completed",
            "source_complete",
        ),
        (
            SubscriptionTermination::ExecutionFailed,
            "failed",
            "execution_failed",
        ),
        (
            SubscriptionTermination::DeliveryFailed,
            "failed",
            "delivery_failed",
        ),
        (SubscriptionTermination::Internal, "failed", "internal"),
        (
            SubscriptionTermination::ClientCancelled,
            "cancelled",
            "client_cancelled",
        ),
        (
            SubscriptionTermination::TransportClosed,
            "cancelled",
            "transport_closed",
        ),
        (
            SubscriptionTermination::ServerShutdown,
            "cancelled",
            "server_shutdown",
        ),
    ] {
        let terminal = value(
            lifecycle::terminal(
                MAX_SAFE_SEQUENCE,
                "Subscription.changed",
                decision,
                MAX_SAFE_SEQUENCE,
            )
            .unwrap(),
        );
        assert_eq!(terminal["outcome"], outcome);
        assert_eq!(terminal["cause"], cause);
        assert_eq!(terminal["request_id"], Value::Null);
        assert_eq!(terminal["subscription_id"], MAX_SAFE_SEQUENCE);
        assert_eq!(terminal["dropped_total"], MAX_SAFE_SEQUENCE);
        let text = terminal.to_string();
        for secret in ["PRIVATE_TOPIC", "PRIVATE_PAYLOAD", "PRIVATE_WIRE_ID"] {
            assert!(!text.contains(secret));
        }
    }
}

#[test]
fn invalid_generation_and_counters_never_serialize() {
    for generation in [0, MAX_SAFE_SEQUENCE + 1] {
        assert!(lifecycle::drop_oldest(generation, "Subscription.changed", 1).is_err());
        assert!(
            lifecycle::terminal(
                generation,
                "Subscription.changed",
                SubscriptionTermination::Internal,
                0,
            )
            .is_err()
        );
    }
    for total in [0, MAX_SAFE_SEQUENCE + 1] {
        assert!(lifecycle::drop_oldest(1, "Subscription.changed", total).is_err());
    }
    assert!(
        lifecycle::terminal(
            1,
            "Subscription.changed",
            SubscriptionTermination::Internal,
            MAX_SAFE_SEQUENCE + 1,
        )
        .is_err()
    );
}
