//! Initial HTTP preparation through actual sequential validation/source workers.

mod admission;
mod cancellation;
mod commit_failures;
mod managed;
mod observation;
mod physical;
mod request_errors;
mod setup_fences;
mod supervision;
pub(super) mod support;
mod terminal;

use std::future::poll_fn;
use std::sync::Arc;

use chigiri_http::{ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::LifecycleResult;

use super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count};
use crate::subscription::driver::SubscriptionOutput;
use crate::transport::streaming::{HttpSubscriptionSignal, PreparedHttpSubscription};
use support::{INVALID, VALID, drain, endpoint, pending, profiles, run, runtime};

#[test]
fn missing_runtime_registration_is_masked_finite_failure_not_cancellation() {
    let runtime = runtime();
    for finite_json in [false, true] {
        let probe = State::new(Vec::new());
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let request = run(
            &runtime,
            pending(
                Arc::clone(&endpoint),
                SubscriptionTransport::ServerSentEvents,
                VALID,
                finite_json,
            ),
        );
        let PreparedHttpSubscription::Response(response) = run(&runtime, request.prepare()) else {
            panic!("missing registration must reject");
        };
        assert_eq!(response.status(), 500);
        assert!(
            std::str::from_utf8(response.as_ref())
                .unwrap()
                .contains("INTERNAL_SERVER_ERROR")
        );
        let expected_media = if finite_json {
            "application/json; charset=utf-8"
        } else {
            "application/graphql-response+json; charset=utf-8"
        };
        assert_eq!(response.headers()[0].value, expected_media);
        assert_eq!(
            signals.completed(),
            [(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"))]
        );
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        drop(response);
        assert_eq!(
            endpoint
                .subscriptions
                .resources
                .delivery()
                .byte_owner()
                .used(),
            0
        );
    }
}

#[test]
fn shared_byte_admission_rejects_before_validation_context_or_source() {
    let runtime = runtime();
    for finite_json in [false, true] {
        let probe = State::new(Vec::new());
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
        let held = bytes
            .try_reserve(
                endpoint
                    .subscriptions
                    .settings
                    .limits()
                    .max_endpoint_encoded_bytes,
            )
            .unwrap();
        let request = run(
            &runtime,
            pending(
                Arc::clone(&endpoint),
                SubscriptionTransport::Multipart,
                INVALID,
                finite_json,
            ),
        );
        let PreparedHttpSubscription::Response(response) = run(&runtime, request.prepare()) else {
            panic!("shared pressure must reject");
        };
        assert_eq!(response.status(), 503);
        assert!(
            std::str::from_utf8(response.as_ref())
                .unwrap()
                .contains("SUBSCRIPTION_LIMIT_EXCEEDED")
        );
        assert!(
            !response
                .headers()
                .iter()
                .any(|header| header.name == "retry-after")
        );
        assert_eq!(
            signals.completed(),
            [(
                LifecycleResult::Rejected,
                Some("SUBSCRIPTION_LIMIT_EXCEEDED")
            )]
        );
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.setup_calls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(
            endpoint
                .in_flight
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert!(
            endpoint
                .subscriptions
                .resources
                .begin_http_subscription()
                .is_ok()
        );
        drop((held, response));
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn joined_validation_error_is_selected_profile_response_without_application_setup() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(Vec::new());
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let request = run(
            &runtime,
            pending(Arc::clone(&endpoint), profile, INVALID, false),
        );
        let PreparedHttpSubscription::Response(response) = run(&runtime, request.prepare()) else {
            panic!("validation error must have no source");
        };
        assert_eq!(response.status(), 200);
        let body = std::str::from_utf8(response.as_ref()).unwrap();
        if profile == SubscriptionTransport::ServerSentEvents {
            assert!(body.starts_with("event: next\ndata: "));
            assert!(body.ends_with("event: complete\ndata: \n\n"));
        } else {
            assert!(body.contains("\"payload\":null,\"errors\":"));
            assert!(body.ends_with("--graphql--"));
            assert!(!body.contains("\"locations\""));
            assert!(!body.contains("\"path\""));
        }
        assert_eq!(count(&probe.counts.factory_calls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 0);
        assert_eq!(count(&probe.counts.completion_calls), 0);
        assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
        let physical = Arc::new(response);
        assert!(bytes.used() > 0);
        let retained = Arc::clone(&physical);
        drop(physical);
        assert!(bytes.used() > 0);
        drop(retained);
        drain(&runtime, guard);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn successful_initial_commit_is_once_only_and_event_demand_keeps_its_reservation() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(7)]);
        let (endpoint, signals) = endpoint(Arc::clone(&probe));
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let request = run(
            &runtime,
            pending(Arc::clone(&endpoint), profile, VALID, false),
        );
        let PreparedHttpSubscription::Ready(ready) = run(&runtime, request.prepare()) else {
            panic!("source must be ready");
        };
        assert_eq!(count(&probe.counts.setup_calls), 1);
        assert_eq!(count(&probe.counts.source_polls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 1);
        assert_eq!(
            *probe.completion_outcomes.lock().unwrap(),
            [ResponseCompletionOutcome::Encoded]
        );
        assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);
        let (initial, mut outbox, mut events) = ready.into_parts();
        assert_eq!(initial.status(), 200);
        assert!(initial.as_ref().is_empty());
        assert!(
            initial
                .headers()
                .iter()
                .any(|header| header.name == "x-subscription" && header.value == "ready")
        );
        assert!(
            events
                .request_next(outbox.reserve_before_poll(0).unwrap())
                .is_ok()
        );
        let HttpSubscriptionSignal::Event {
            reservation,
            output: SubscriptionOutput::Event(response),
        } = run(&runtime, poll_fn(|context| events.poll_worker(context)))
        else {
            panic!("one demanded event");
        };
        outbox
            .encode_and_enqueue(reservation, &response, 0)
            .unwrap();
        let (frame, mut progress) = outbox.begin_send(0).unwrap().unwrap();
        assert!(
            std::str::from_utf8(frame.as_bytes())
                .unwrap()
                .contains("\"value\":7")
        );
        assert!(progress.accept(frame.len(), 0).unwrap());
        drop(frame);
        outbox.finish_send(progress, 0).unwrap();
        assert_eq!(count(&probe.counts.setup_calls), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        drop((initial, outbox, events));
        drain(&runtime, guard);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(
            endpoint
                .subscriptions
                .resources
                .delivery()
                .byte_owner()
                .used(),
            0
        );
    }
}

#[test]
fn source_error_and_context_rejection_do_not_deadlock_on_initial_command_or_join() {
    let runtime = runtime();
    for (setup, factory, status, completions) in [
        (SetupMode::OuterError, FactoryMode::Ready, 200, 1),
        (SetupMode::Ready, FactoryMode::Reject, 403, 0),
    ] {
        let probe = State::modes(
            Vec::new(),
            setup,
            EventMode::Object,
            factory,
            FactoryMode::Ready,
            HookMode::Ready,
        );
        let (endpoint, _) = endpoint(Arc::clone(&probe));
        let guard = endpoint
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let request = run(
            &runtime,
            pending(
                Arc::clone(&endpoint),
                SubscriptionTransport::Multipart,
                VALID,
                false,
            ),
        );
        let PreparedHttpSubscription::Response(response) = run(&runtime, request.prepare()) else {
            panic!("setup rejected without live delivery");
        };
        assert_eq!(response.status(), status);
        assert_eq!(count(&probe.counts.completion_calls), completions);
        drop(response);
        drain(&runtime, guard);
        assert_eq!(
            endpoint
                .subscriptions
                .resources
                .delivery()
                .byte_owner()
                .used(),
            0
        );
    }
}

#[test]
fn unpolled_preparation_drop_is_genuine_cancelled_with_zero_application_work() {
    let runtime = runtime();
    let probe = State::new(Vec::new());
    let (endpoint, signals) = endpoint(Arc::clone(&probe));
    let request = run(
        &runtime,
        pending(
            Arc::clone(&endpoint),
            SubscriptionTransport::ServerSentEvents,
            VALID,
            false,
        ),
    );
    drop(request.prepare());
    assert_eq!(signals.completed(), [(LifecycleResult::Cancelled, None)]);
    assert_eq!(count(&probe.counts.factory_calls), 0);
    assert_eq!(count(&probe.counts.setup_calls), 0);
    assert_eq!(
        endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner()
            .used(),
        0
    );
}
