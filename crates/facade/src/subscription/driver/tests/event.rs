//! Event demand, fresh-context, and cancellation integration tests.

use std::sync::Arc;

use chigiri_http::{RequestPhase, ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::Value;

use super::super::{EndpointSubscription, SubscriptionInitialization, SubscriptionOutput};
use super::deadlines::{CONTEXT_TIMEOUT, Deadlines, EXECUTION_TIMEOUT, SecondPollDeadline};
use super::harness::{
    block_on, endpoint, initialize, poll_once, query_request, request, response_parts,
};
use super::state::{
    AppContext, ContextKind, EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count,
};

const METADATA_LIMIT: usize = 16 * 1024;

#[test]
fn idle_source_cleanup_retains_owned_source_until_the_cleanup_is_polled() {
    let probe = State::new(vec![SourceStep::Pending]);
    let subscription = ready(Arc::clone(&probe), SubscriptionTransport::WebSocket);
    let cleanup = subscription.into_cleanup();
    assert_eq!(count(&probe.counts.source_drops), 0);
    assert_eq!(count(&probe.counts.source_polls), 0);
    block_on(cleanup);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(count(&probe.counts.source_polls), 0);
}

#[test]
fn live_source_does_not_retain_the_endpoint_resource_or_quarantine_graph() {
    let probe = State::new(vec![SourceStep::Event(17)]);
    let state = endpoint(Arc::clone(&probe), METADATA_LIMIT);
    let weak = Arc::downgrade(&state);
    let initialized = block_on(initialize(
        Arc::clone(&state),
        request(),
        SubscriptionTransport::WebSocket,
        &Deadlines(Vec::new()),
    ));
    let SubscriptionInitialization::Ready {
        mut subscription, ..
    } = initialized
    else {
        panic!("source must initialize before checking detached graph ownership");
    };
    drop(state);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        event_value(&block_on(subscription.next(&Deadlines(Vec::new())))),
        Some(17)
    );
    drop(subscription);
    assert_eq!(count(&probe.counts.source_drops), 1);
}

#[test]
fn two_events_demand_once_each_and_use_fresh_contexts_and_identities() {
    let probe = State::new(vec![SourceStep::Event(10), SourceStep::Event(20)]);
    let mut subscription = ready(Arc::clone(&probe), SubscriptionTransport::ServerSentEvents);
    assert_eq!(count(&probe.counts.source_polls), 0);

    let first = block_on(subscription.next(&Deadlines(Vec::new())));
    assert_eq!(event_value(&first), Some(10));
    assert_eq!(count(&probe.counts.source_polls), 1);
    assert_eq!(count(&probe.counts.event_context_drops), 1);
    assert_eq!(count(&probe.counts.event_state_drops), 1);

    let second = block_on(subscription.next(&Deadlines(Vec::new())));
    assert_eq!(event_value(&second), Some(20));
    assert_eq!(count(&probe.counts.source_polls), 2);
    assert_eq!(count(&probe.counts.event_context_drops), 2);
    assert_eq!(count(&probe.counts.event_state_drops), 2);
    assert_eq!(count(&probe.counts.factory_calls), 3);
    assert_eq!(count(&probe.counts.hook_calls), 1);
    assert_eq!(count(&probe.counts.completion_calls), 1);
    assert_eq!(*probe.event_generations.lock().unwrap(), [2, 3]);
    assert_eq!(*probe.retained_arguments.lock().unwrap(), [2, 2]);
    assert_eq!(
        *probe.phase_request_ids.lock().unwrap(),
        [
            Some("request-1".to_owned()),
            Some("request-2".to_owned()),
            Some("request-3".to_owned()),
        ]
    );
    assert_eq!(
        *probe.phases.lock().unwrap(),
        [
            RequestPhase::SubscriptionSetup {
                transport: SubscriptionTransport::ServerSentEvents,
            },
            RequestPhase::SubscriptionEvent {
                transport: SubscriptionTransport::ServerSentEvents,
                sequence: 1,
            },
            RequestPhase::SubscriptionEvent {
                transport: SubscriptionTransport::ServerSentEvents,
                sequence: 2,
            },
        ]
    );

    assert!(matches!(
        block_on(subscription.next(&Deadlines(Vec::new()))),
        SubscriptionOutput::Complete
    ));
    assert_eq!(count(&probe.counts.source_polls), 3);
    assert_eq!(count(&probe.counts.source_drops), 1);
}

#[test]
fn dropping_next_before_first_poll_revokes_the_source_without_demand() {
    let probe = State::new(vec![SourceStep::Event(1)]);
    let mut subscription = ready(Arc::clone(&probe), SubscriptionTransport::WebSocket);
    let deadlines = Deadlines(Vec::new());
    let next = subscription.next(&deadlines);
    assert_eq!(count(&probe.counts.source_polls), 0);
    assert_eq!(count(&probe.counts.source_drops), 0);
    drop(next);
    assert_eq!(count(&probe.counts.source_drops), 1);
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.hook_calls), 0);
    assert_eq!(count(&probe.counts.completion_calls), 0);
}

#[test]
fn dropping_pending_source_or_event_future_releases_every_owner() {
    let pending_source = State::new(vec![SourceStep::Pending]);
    let mut subscription = ready(
        Arc::clone(&pending_source),
        SubscriptionTransport::ServerSentEvents,
    );
    let deadlines = Deadlines(Vec::new());
    let mut next = subscription.next(&deadlines);
    assert!(poll_once(next.as_mut()).is_pending());
    assert_eq!(count(&pending_source.counts.source_polls), 1);
    assert_eq!(count(&pending_source.counts.factory_calls), 1);
    drop(next);
    assert_eq!(count(&pending_source.counts.source_drops), 1);

    let pending_event = State::modes(
        vec![SourceStep::Event(1)],
        SetupMode::Ready,
        EventMode::Pending,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let mut subscription = ready(Arc::clone(&pending_event), SubscriptionTransport::Multipart);
    let mut next = subscription.next(&deadlines);
    assert!(poll_once(next.as_mut()).is_pending());
    drop(next);
    assert_eq!(count(&pending_event.counts.event_future_drops), 1);
    assert_eq!(count(&pending_event.counts.event_context_drops), 1);
    assert_eq!(count(&pending_event.counts.event_state_drops), 1);
    assert_eq!(count(&pending_event.counts.source_drops), 1);
}

#[test]
fn event_context_failures_and_deadlines_are_terminal_and_drop_the_source() {
    for (mode, timeout, expected_code) in [
        (FactoryMode::Reject, None, None),
        (FactoryMode::Internal, None, Some("INTERNAL_SERVER_ERROR")),
        (
            FactoryMode::Pending,
            Some(CONTEXT_TIMEOUT),
            Some("REQUEST_TIMEOUT"),
        ),
    ] {
        let probe = State::modes(
            vec![SourceStep::Event(1)],
            SetupMode::Ready,
            EventMode::Object,
            FactoryMode::Ready,
            mode,
            HookMode::Ready,
        );
        let mut subscription = ready(Arc::clone(&probe), SubscriptionTransport::Multipart);
        let output = match timeout {
            Some(duration) => block_on(subscription.next(&SecondPollDeadline(duration))),
            None => block_on(subscription.next(&Deadlines(Vec::new()))),
        };
        let SubscriptionOutput::Terminal(response) = output else {
            panic!("event context failure must terminate the generation");
        };
        if let Some(expected) = expected_code {
            assert_eq!(code(&response), Some(expected));
        } else {
            assert_eq!(response.errors[0].message, "context denied");
        }
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(count(&probe.counts.hook_calls), 1);
        if mode == FactoryMode::Pending {
            assert_eq!(count(&probe.counts.factory_future_drops), 1);
        }
    }
}

#[test]
fn event_execution_timeout_drops_pending_mapper_context_and_source() {
    let probe = State::modes(
        vec![SourceStep::Event(1)],
        SetupMode::Ready,
        EventMode::Pending,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let mut subscription = ready(Arc::clone(&probe), SubscriptionTransport::Multipart);
    let output = block_on(subscription.next(&SecondPollDeadline(EXECUTION_TIMEOUT)));
    let SubscriptionOutput::Terminal(response) = output else {
        panic!("event execution timeout must terminate the generation");
    };
    assert_eq!(code(&response), Some("REQUEST_TIMEOUT"));
    assert_eq!(count(&probe.counts.event_future_drops), 1);
    assert_eq!(count(&probe.counts.event_context_drops), 1);
    assert_eq!(count(&probe.counts.event_state_drops), 1);
    assert_eq!(count(&probe.counts.source_drops), 1);
}

#[test]
fn source_error_and_framework_invariant_are_masked_terminal_results() {
    let source_error = State::new(vec![SourceStep::Error]);
    let mut subscription = ready(
        Arc::clone(&source_error),
        SubscriptionTransport::ServerSentEvents,
    );
    let SubscriptionOutput::Terminal(response) =
        block_on(subscription.next(&Deadlines(Vec::new())))
    else {
        panic!("source error must terminate the generation");
    };
    assert_eq!(code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert_eq!(count(&source_error.counts.source_maps), 1);
    assert_eq!(count(&source_error.counts.factory_calls), 1);

    let invariant = State::modes(
        vec![SourceStep::Event(1)],
        SetupMode::Ready,
        EventMode::FrameworkInvariant,
        FactoryMode::Ready,
        FactoryMode::Ready,
        HookMode::Ready,
    );
    let mut subscription = ready(Arc::clone(&invariant), SubscriptionTransport::Multipart);
    let SubscriptionOutput::Terminal(response) =
        block_on(subscription.next(&Deadlines(Vec::new())))
    else {
        panic!("framework invariant must terminate the generation");
    };
    assert_eq!(code(&response), Some("INTERNAL_SERVER_ERROR"));
    assert_eq!(count(&invariant.counts.source_drops), 1);
}

#[test]
fn ordinary_finite_execution_remains_independent_of_subscription_setup() {
    let probe = State::new(Vec::new());
    let state = endpoint(Arc::clone(&probe), METADATA_LIMIT);
    let context = AppContext {
        generation: 99,
        kind: ContextKind::Event,
        state: Arc::clone(&probe),
    };
    let response = block_on(state.schema.execute(query_request(), context));
    assert!(response.errors.is_empty());
    assert_eq!(count(&probe.counts.setup_calls), 0);
    assert_eq!(count(&probe.counts.factory_calls), 0);
}

fn ready(probe: Arc<State>, profile: SubscriptionTransport) -> EndpointSubscription<AppContext> {
    let result = block_on(initialize(
        endpoint(probe, METADATA_LIMIT),
        request(),
        profile,
        &Deadlines(Vec::new()),
    ));
    let SubscriptionInitialization::Ready {
        subscription,
        completion,
        ..
    } = result
    else {
        panic!("source setup failed");
    };
    let _ = completion.finish(response_parts(), ResponseCompletionOutcome::Encoded);
    *subscription
}

fn event_value(output: &SubscriptionOutput) -> Option<i128> {
    let SubscriptionOutput::Event(response) = output else {
        return None;
    };
    let Value::Object(root) = response.data.as_ref()? else {
        return None;
    };
    let Value::Object(event) = root.get("chosen")? else {
        return None;
    };
    let Value::Int(value) = event.get("value")? else {
        return None;
    };
    Some(*value)
}

fn code(response: &chigiri_runtime::GraphQlResponse) -> Option<&str> {
    let Value::String(code) = response.errors.first()?.extensions.get("code")? else {
        return None;
    };
    Some(code)
}
