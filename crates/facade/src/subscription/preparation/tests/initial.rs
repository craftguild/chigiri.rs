use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chigiri_http::{ResponseCompletionOutcome, SubscriptionTransport};
use chigiri_runtime::{GraphQlError, GraphQlResponse};

use super::super::initial::{InitialResponsePreparation, prepare_initial_response};
use super::fixture::{Counts, Hook, HookMode, HookState, fresh, parts};
use super::support::{ImmediateDeadline, NeverDeadline, block_on, overall, request, response};
use crate::transport::erase_hook;

#[test]
fn http_hook_observes_dropped_setup_context_and_completion_finishes_once() {
    let (_, request) = request(SubscriptionTransport::ServerSentEvents);
    let counts = Arc::new(Counts::default());
    let hook = erase_hook::<HookState, _>(Hook {
        counts: Arc::clone(&counts),
        mode: HookMode::Ready,
    });
    let mapped = GraphQlResponse::request_error(GraphQlError::new("safe source error"));
    let result = prepare(
        fresh(Arc::clone(&counts)),
        &request,
        &mapped,
        Some(&hook),
        &NeverDeadline,
    );
    let InitialResponsePreparation::Ready(prepared) = result else {
        panic!("HTTP initial hook must succeed");
    };
    assert_eq!(counts.context_drops.load(Ordering::SeqCst), 1);
    assert_eq!(counts.state_drops.load(Ordering::SeqCst), 1);
    assert_eq!(counts.hook_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*counts.response_error_counts.lock().unwrap(), vec![1]);
    assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 0);

    let (headers, completion, telemetry) = prepared.into_parts();
    assert_eq!(headers.__into_values()[0].name, "x-subscription");
    assert!(telemetry.request_id().is_none());
    let completed = completion.finish(parts(), ResponseCompletionOutcome::Encoded);
    assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counts.completion_drops.load(Ordering::SeqCst), 1);
    assert_eq!(completed.headers[0].value, "Encoded");
}

#[test]
fn absent_hook_still_transfers_the_single_initial_completion_owner() {
    let (_, request) = request(SubscriptionTransport::Multipart);
    let counts = Arc::new(Counts::default());
    let result = prepare(
        fresh(Arc::clone(&counts)),
        &request,
        &response(),
        None,
        &NeverDeadline,
    );
    let InitialResponsePreparation::Ready(prepared) = result else {
        panic!("absent hook must produce empty additions");
    };
    let (headers, completion, _) = prepared.into_parts();
    assert!(headers.__into_values().is_empty());
    let completed = completion.finish(parts(), ResponseCompletionOutcome::EncodingLimitExceeded);
    assert_eq!(counts.hook_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 1);
    assert_eq!(completed.headers[0].value, "EncodingLimitExceeded");
}

#[test]
fn hook_failure_and_timeout_keep_completion_on_the_same_finish_path() {
    for (mode, immediate, expected) in [
        (HookMode::Internal, false, ResponseCompletionOutcome::Failed),
        (HookMode::Pending, true, ResponseCompletionOutcome::TimedOut),
    ] {
        let (_, request) = request(SubscriptionTransport::ServerSentEvents);
        let counts = Arc::new(Counts::default());
        let hook = erase_hook::<HookState, _>(Hook {
            counts: Arc::clone(&counts),
            mode,
        });
        let result = if immediate {
            prepare(
                fresh(Arc::clone(&counts)),
                &request,
                &response(),
                Some(&hook),
                &ImmediateDeadline,
            )
        } else {
            prepare(
                fresh(Arc::clone(&counts)),
                &request,
                &response(),
                Some(&hook),
                &NeverDeadline,
            )
        };
        let completion = match result {
            InitialResponsePreparation::HookFailed {
                error,
                completion,
                telemetry,
            } => {
                assert_eq!(format!("{error:?}"), "HookError");
                assert!(telemetry.request_id().is_none());
                completion
            }
            InitialResponsePreparation::TimedOut {
                completion,
                telemetry,
            } => {
                assert!(telemetry.request_id().is_none());
                completion
            }
            _ => panic!("initial hook failure classification changed"),
        };
        assert_eq!(counts.context_drops.load(Ordering::SeqCst), 1);
        assert_eq!(counts.state_drops.load(Ordering::SeqCst), 1);
        assert_eq!(counts.hook_calls.load(Ordering::SeqCst), 1);
        assert_eq!(*counts.response_error_counts.lock().unwrap(), vec![0]);
        let completed = completion.finish(parts(), expected);
        assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 1);
        assert_eq!(completed.headers[0].value, format!("{expected:?}"));
        if immediate {
            assert_eq!(counts.future_drops.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn websocket_and_event_ownership_drop_without_hook_or_completion() {
    let (_, request) = request(SubscriptionTransport::WebSocket);
    let counts = Arc::new(Counts::default());
    let hook = erase_hook::<HookState, _>(Hook {
        counts: Arc::clone(&counts),
        mode: HookMode::Ready,
    });
    let result = prepare(
        fresh(Arc::clone(&counts)),
        &request,
        &response(),
        Some(&hook),
        &NeverDeadline,
    );
    let InitialResponsePreparation::WebSocket { telemetry } = result else {
        panic!("WebSocket must not prepare an HTTP response");
    };
    assert!(telemetry.request_id().is_none());
    assert_eq!(counts.context_drops.load(Ordering::SeqCst), 1);
    assert_eq!(counts.state_drops.load(Ordering::SeqCst), 1);
    assert_eq!(counts.hook_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.completion_drops.load(Ordering::SeqCst), 1);

    let event = fresh(Arc::clone(&counts));
    drop(event);
    assert_eq!(counts.context_drops.load(Ordering::SeqCst), 2);
    assert_eq!(counts.state_drops.load(Ordering::SeqCst), 2);
    assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.completion_drops.load(Ordering::SeqCst), 2);
}

#[test]
fn source_setup_failure_drops_context_and_state_before_finishing_http_only() {
    let http_counts = Arc::new(Counts::default());
    let completion =
        fresh(Arc::clone(&http_counts)).into_failure_completion(SubscriptionTransport::Multipart);
    assert_eq!(http_counts.context_drops.load(Ordering::SeqCst), 1);
    assert_eq!(http_counts.state_drops.load(Ordering::SeqCst), 1);
    assert_eq!(http_counts.completion_calls.load(Ordering::SeqCst), 0);
    let completed = completion.finish(parts(), ResponseCompletionOutcome::TimedOut);
    assert_eq!(http_counts.completion_calls.load(Ordering::SeqCst), 1);
    assert_eq!(completed.headers[0].value, "TimedOut");

    let websocket_counts = Arc::new(Counts::default());
    let completion = fresh(Arc::clone(&websocket_counts))
        .into_failure_completion(SubscriptionTransport::WebSocket);
    assert_eq!(websocket_counts.context_drops.load(Ordering::SeqCst), 1);
    assert_eq!(websocket_counts.state_drops.load(Ordering::SeqCst), 1);
    assert_eq!(websocket_counts.completion_drops.load(Ordering::SeqCst), 1);
    let completed = completion.finish(parts(), ResponseCompletionOutcome::Failed);
    assert!(completed.headers.is_empty());
    assert_eq!(websocket_counts.completion_calls.load(Ordering::SeqCst), 0);
}

fn prepare<C>(
    fresh: super::super::FreshContext<C>,
    request: &crate::subscription::request::SubscriberRequest,
    response: &GraphQlResponse,
    hook: Option<&Arc<dyn crate::transport::ErasedResponseHook>>,
    deadlines: &dyn chigiri_http::DeadlineFactory,
) -> InitialResponsePreparation {
    let mut overall = overall();
    block_on(prepare_initial_response(
        fresh,
        request,
        response,
        hook,
        deadlines,
        Duration::from_secs(1),
        overall.as_mut(),
    ))
}
