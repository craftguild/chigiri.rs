//! Fresh event-local state and deliberate shared authorization integration.

mod support;

use chigiri_http::RequestPhase;

use super::super::SubscriptionOutput;
use support::{Fixture, error_code, event_value};

#[test]
fn revoked_original_credential_requires_a_new_subscription_for_a_new_token() {
    let fixture = Fixture::new();
    let mut original = fixture.subscribe("token-a", 2, [11, 12]);

    assert_eq!(event_value(&original.next()), Some(11));
    fixture.revoke("token-a");
    let SubscriptionOutput::Terminal(response) = original.next() else {
        panic!("revoked original credentials must terminate before root execution");
    };
    assert_eq!(error_code(&response), Some("UNAUTHENTICATED"));
    drop(original);

    let mut replacement = fixture.subscribe("token-b", 2, [21]);
    assert_eq!(event_value(&replacement.next()), Some(21));
    assert!(matches!(replacement.next(), SubscriptionOutput::Complete));
    drop(replacement);

    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.setup_tokens, ["token-a", "token-b"]);
    assert_eq!(
        snapshot.credentials,
        ["token-a", "token-a", "token-a", "token-b", "token-b"]
    );
    assert_eq!(snapshot.root_arguments, [2, 2]);
    assert_eq!(snapshot.nested_values, [11, 21]);
    assert_eq!(snapshot.setup_calls, 2);
    assert_eq!(snapshot.source_polls, 4);
    assert_eq!(snapshot.source_drops, 2);
    assert_eq!(snapshot.mapper_calls, 0);
    assert_eq!(snapshot.hook_calls, 2);
    assert_eq!(snapshot.completion_calls, 2);
    assert_eq!(snapshot.request_ids.len(), 5);
    assert_eq!(snapshot.local_starts.len(), 2);
    assert_eq!(snapshot.context_drops, 4);
    assert!(
        snapshot
            .credentials
            .iter()
            .all(|token| token != "publisher")
    );
}

#[test]
fn current_root_argument_policy_uses_fresh_local_state_and_shared_services() {
    let fixture = Fixture::new();
    let mut subscription = fixture.subscribe("token-a", 2, [31, 32, 33]);

    assert_eq!(event_value(&subscription.next()), Some(31));
    fixture.permit_limit(9);
    let denied = subscription.next();
    let SubscriptionOutput::Event(response) = denied else {
        panic!("root authorization is an ordinary event result, not a terminal");
    };
    assert_eq!(error_code(&response), Some("FORBIDDEN"));

    fixture.permit_limit(2);
    assert_eq!(event_value(&subscription.next()), Some(33));
    assert!(matches!(subscription.next(), SubscriptionOutput::Complete));
    drop(subscription);

    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.setup_calls, 1);
    assert_eq!(snapshot.setup_tokens, ["token-a"]);
    assert_eq!(snapshot.credentials, ["token-a"; 4]);
    assert_eq!(snapshot.root_arguments, [2, 2, 2]);
    assert_eq!(snapshot.policy_revisions, [0, 1, 2]);
    assert_eq!(snapshot.local_starts, [(0, 1, true); 3]);
    assert_eq!(snapshot.nested_values, [31, 33]);
    assert_eq!(snapshot.source_polls, 4);
    assert_eq!(snapshot.source_drops, 1);
    assert_eq!(snapshot.mapper_calls, 0);
    assert_eq!(snapshot.hook_calls, 1);
    assert_eq!(snapshot.completion_calls, 1);
    assert_eq!(snapshot.request_ids.len(), 4);
    assert_eq!(snapshot.context_drops, 4);
    assert_eq!(
        snapshot
            .phases
            .iter()
            .filter(|phase| matches!(phase, RequestPhase::SubscriptionEvent { .. }))
            .count(),
        3
    );
}
