//! Setup, initial-hook, and pre-source failure integration tests.

use std::sync::Arc;

use chigiri_http::{ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::{GraphQlRequest, Value};

use super::super::SubscriptionInitialization;
use super::deadlines::{
    CONTEXT_TIMEOUT, Deadlines, EXECUTION_TIMEOUT, HOOK_TIMEOUT, SecondPollDeadline,
};
use super::harness::{
    block_on, endpoint, initialize, initialize_with_deadline, invalid_request, poll_once, request,
    response_parts,
};
use super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count};

const METADATA_LIMIT: usize = 16 * 1024;

#[test]
fn preflight_and_metadata_limits_run_before_factory_and_source_setup() {
    let invalid = State::new(Vec::new());
    let result = block_on(initialize(
        endpoint(Arc::clone(&invalid), METADATA_LIMIT),
        invalid_request(),
        SubscriptionTransport::ServerSentEvents,
        &Deadlines(Vec::new()),
    ));
    assert!(matches!(
        result,
        SubscriptionInitialization::Response { .. }
    ));
    assert_eq!(count(&invalid.counts.factory_calls), 0);
    assert_eq!(count(&invalid.counts.setup_calls), 0);

    let oversized = State::new(Vec::new());
    let result = block_on(initialize(
        endpoint(Arc::clone(&oversized), 1),
        request(),
        SubscriptionTransport::Multipart,
        &Deadlines(Vec::new()),
    ));
    let SubscriptionInitialization::Response { response, .. } = result else {
        panic!("metadata limit must return a pre-source GraphQL response");
    };
    assert_eq!(code(&response), Some("BAD_USER_INPUT"));
    assert_eq!(count(&oversized.counts.factory_calls), 0);
    assert_eq!(count(&oversized.counts.setup_calls), 0);
    assert_eq!(count(&oversized.counts.hook_calls), 0);

    let missing_variable = State::new(Vec::new());
    let request =
        GraphQlRequest::new("subscription Named($limit: Int!) { events(limit: $limit) { value } }")
            .with_operation_name("Named");
    let result = block_on(initialize(
        endpoint(Arc::clone(&missing_variable), METADATA_LIMIT),
        request,
        SubscriptionTransport::Multipart,
        &Deadlines(Vec::new()),
    ));
    assert!(matches!(
        result,
        SubscriptionInitialization::Response { .. }
    ));
    assert_eq!(count(&missing_variable.counts.factory_calls), 0);
    assert_eq!(count(&missing_variable.counts.setup_calls), 0);
}

#[test]
fn successful_http_setup_hooks_once_without_prefetch_and_finishes_once() {
    let probe = State::new(vec![SourceStep::Event(7)]);
    let result = block_on(initialize(
        endpoint(Arc::clone(&probe), METADATA_LIMIT),
        request(),
        SubscriptionTransport::ServerSentEvents,
        &Deadlines(Vec::new()),
    ));
    let SubscriptionInitialization::Ready {
        subscription,
        headers,
        completion,
    } = result
    else {
        panic!("source setup must establish the owner");
    };
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.source_polls), 0);
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.setup_context_drops), 1);
    assert_eq!(count(&probe.counts.setup_state_drops), 1);
    assert_eq!(count(&probe.counts.hook_calls), 1);
    assert_eq!(*probe.hook_error_counts.lock().unwrap(), [0]);
    assert_eq!(headers.__into_values()[0].name, "x-subscription".to_owned());
    assert!(subscription.overflow().is_some());
    assert_eq!(count(&probe.counts.completion_calls), 0);
    let _ = completion.finish(response_parts(), ResponseCompletionOutcome::Encoded);
    assert_eq!(count(&probe.counts.completion_calls), 1);
    assert_eq!(
        *probe.completion_outcomes.lock().unwrap(),
        [ResponseCompletionOutcome::Encoded]
    );
}

#[test]
fn source_setup_error_runs_the_http_hook_with_the_mapped_response() {
    let probe = State::modes(
        Vec::new(),
        SetupMode::OuterError,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let result = block_on(initialize(
        endpoint(Arc::clone(&probe), METADATA_LIMIT),
        request(),
        SubscriptionTransport::Multipart,
        &Deadlines(Vec::new()),
    ));
    let SubscriptionInitialization::Response {
        response,
        completion,
        ..
    } = result
    else {
        panic!("setup error must remain a selected-profile response");
    };
    assert_eq!(code(&response), Some("SOURCE_DENIED"));
    assert_eq!(count(&probe.counts.hook_calls), 1);
    assert_eq!(*probe.hook_error_counts.lock().unwrap(), [1]);
    let _ = completion.finish(response_parts(), ResponseCompletionOutcome::Encoded);
    assert_eq!(count(&probe.counts.completion_calls), 1);
    assert_eq!(count(&probe.counts.source_drops), 0);
}

#[test]
fn hook_failure_and_timeout_drop_the_live_source_and_complete_once() {
    for (hook_mode, deadlines, outcome) in [
        (
            HookMode::Internal,
            Deadlines(Vec::new()),
            ResponseCompletionOutcome::Failed,
        ),
        (
            HookMode::Pending,
            Deadlines(vec![HOOK_TIMEOUT]),
            ResponseCompletionOutcome::TimedOut,
        ),
    ] {
        let probe = State::modes(
            vec![SourceStep::Event(1)],
            SetupMode::Ready,
            EventMode::Object,
            FactoryMode::Ready,
            FactoryMode::Ready,
            hook_mode,
        );
        let result = block_on(initialize(
            endpoint(Arc::clone(&probe), METADATA_LIMIT),
            request(),
            SubscriptionTransport::ServerSentEvents,
            &deadlines,
        ));
        let SubscriptionInitialization::Rejected(response) = result else {
            panic!("initial hook failure must reject before stream commit");
        };
        assert!(matches!(response.status, 500 | 504));
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(count(&probe.counts.hook_calls), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(*probe.completion_outcomes.lock().unwrap(), [outcome]);
        if hook_mode == HookMode::Pending {
            assert_eq!(count(&probe.counts.hook_future_drops), 1);
        }
    }
}

#[test]
fn pending_source_initializer_timeout_finishes_http_but_not_websocket_completion() {
    for profile in [
        SubscriptionTransport::ServerSentEvents,
        SubscriptionTransport::WebSocket,
    ] {
        let probe = State::modes(
            Vec::new(),
            SetupMode::Pending,
            EventMode::Object,
            FactoryMode::Ready,
            FactoryMode::Ready,
            HookMode::Ready,
        );
        let result = block_on(initialize(
            endpoint(Arc::clone(&probe), METADATA_LIMIT),
            request(),
            profile,
            &SecondPollDeadline(EXECUTION_TIMEOUT),
        ));
        assert!(matches!(result, SubscriptionInitialization::Rejected(_)));
        assert_eq!(count(&probe.counts.setup_calls), 1);
        assert_eq!(count(&probe.counts.setup_future_drops), 1);
        assert_eq!(count(&probe.counts.setup_context_drops), 1);
        assert_eq!(count(&probe.counts.setup_state_drops), 1);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        let expected = usize::from(profile != SubscriptionTransport::WebSocket);
        assert_eq!(count(&probe.counts.completion_calls), expected);
        if expected == 1 {
            assert_eq!(
                *probe.completion_outcomes.lock().unwrap(),
                [ResponseCompletionOutcome::TimedOut]
            );
        }
    }
}

#[test]
fn context_rejection_internal_error_and_timeout_never_reach_source_setup() {
    for (factory, deadline, status) in [
        (FactoryMode::Reject, None, 403),
        (FactoryMode::Internal, None, 500),
        (FactoryMode::Pending, Some(CONTEXT_TIMEOUT), 504),
    ] {
        let probe = State::modes(
            Vec::new(),
            SetupMode::Ready,
            EventMode::Object,
            factory,
            FactoryMode::Ready,
            HookMode::Ready,
        );
        let deadlines: Box<dyn chigiri_http::DeadlineFactory> = match deadline {
            Some(duration) => Box::new(SecondPollDeadline(duration)),
            None => Box::new(Deadlines(Vec::new())),
        };
        let result = block_on(initialize(
            endpoint(Arc::clone(&probe), METADATA_LIMIT),
            request(),
            SubscriptionTransport::Multipart,
            deadlines.as_ref(),
        ));
        let SubscriptionInitialization::Rejected(response) = result else {
            panic!("context preparation failure must reject before source setup");
        };
        assert_eq!(response.status, status);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        if factory == FactoryMode::Pending {
            assert_eq!(count(&probe.counts.factory_future_drops), 1);
        }
    }
}

#[test]
fn dropping_setup_before_or_during_poll_releases_every_created_owner() {
    let before_poll = State::modes(
        Vec::new(),
        SetupMode::Pending,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let deadlines = Deadlines(Vec::new());
    let future = initialize(
        endpoint(Arc::clone(&before_poll), METADATA_LIMIT),
        request(),
        SubscriptionTransport::ServerSentEvents,
        &deadlines,
    );
    drop(future);
    assert_eq!(count(&before_poll.counts.factory_calls), 0);
    assert_eq!(count(&before_poll.counts.setup_calls), 0);

    let during_setup = State::modes(
        Vec::new(),
        SetupMode::Pending,
        EventMode::Object,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let mut future = Box::pin(initialize(
        endpoint(Arc::clone(&during_setup), METADATA_LIMIT),
        request(),
        SubscriptionTransport::Multipart,
        &deadlines,
    ));
    assert!(poll_once(future.as_mut()).is_pending());
    assert_eq!(count(&during_setup.counts.factory_calls), 1);
    assert_eq!(count(&during_setup.counts.setup_calls), 1);
    drop(future);
    assert_eq!(count(&during_setup.counts.setup_future_drops), 1);
    assert_eq!(count(&during_setup.counts.setup_context_drops), 1);
    assert_eq!(count(&during_setup.counts.setup_state_drops), 1);
    assert_eq!(count(&during_setup.counts.completion_calls), 0);
    assert_eq!(count(&during_setup.counts.completion_drops), 1);
}

#[test]
fn inherited_overall_deadline_is_not_restarted_for_subscription_setup() {
    let probe = State::new(Vec::new());
    let deadlines = Deadlines(Vec::new());
    let result = block_on(initialize_with_deadline(
        endpoint(Arc::clone(&probe), METADATA_LIMIT),
        request(),
        SubscriptionTransport::ServerSentEvents,
        &deadlines,
        async {},
    ));
    let SubscriptionInitialization::Rejected(response) = result else {
        panic!("an inherited expired budget must reject setup");
    };
    assert_eq!(response.status, 504);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
    assert_eq!(count(&probe.counts.hook_calls), 0);
}

fn code(response: &chigiri_runtime::GraphQlResponse) -> Option<&str> {
    let Value::String(code) = response.errors.first()?.extensions.get("code")? else {
        return None;
    };
    Some(code)
}
