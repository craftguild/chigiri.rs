//! Actual request-error preparation and validated finite/profile classification.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chigiri_http::{
    BoxFuture, HookError, ResponseCompletionOutcome, ResponseHeaders, ResponseHook,
    ResponseHookContext, SubscriptionTransport,
};

use super::super::state::{HookState, State, count};
use super::support::{VALID, drain, endpoint, pending, profiles, run, runtime};
use crate::transport::streaming::PreparedHttpSubscription;

const REQUEST_ERRORS: &[(&[u8], &str)] = &[
    (br#"{"query":"subscription {"}"#, "GRAPHQL_PARSE_FAILED"),
    (br#"{"query":"subscription A { events(limit: 2) { value } } subscription B { events(limit: 2) { value } }"}"#, "OPERATION_NOT_FOUND"),
    (br#"{"query":"subscription Named { events(limit: 2) { value } }","operationName":"missing"}"#, "OPERATION_NOT_FOUND"),
    (br#"{"query":"{ value } subscription Named { events(limit: 2) { value } }","operationName":"Named"}"#, "GRAPHQL_VALIDATION_FAILED"),
];

#[test]
fn stream_only_request_errors_keep_the_profile_without_context_source_hook_or_completion() {
    let runtime = runtime();
    for profile in profiles() {
        for (body, code) in REQUEST_ERRORS {
            let probe = State::new(Vec::new());
            let (endpoint, signals) = endpoint(Arc::clone(&probe));
            let bytes = endpoint.subscriptions.resources.delivery().byte_owner();
            let guard = endpoint
                .subscriptions
                .resources
                .register_worker_runtime()
                .unwrap();
            let pending = run(
                &runtime,
                pending(Arc::clone(&endpoint), profile, body, false),
            );
            let PreparedHttpSubscription::Response(response) = run(&runtime, pending.prepare())
            else {
                panic!("a request error cannot create a source or close without a response");
            };
            assert_eq!(response.status(), 200);
            assert!(response.is_streaming_profile());
            let text = std::str::from_utf8(response.as_ref()).unwrap();
            assert!(text.contains(code), "{text}");
            match profile {
                SubscriptionTransport::ServerSentEvents => {
                    assert!(text.starts_with("event: next\ndata: "));
                    assert!(text.ends_with("event: complete\ndata: \n\n"));
                }
                SubscriptionTransport::Multipart => {
                    assert!(text.contains("\"payload\":null,\"errors\":"));
                    assert!(!text.contains("\"locations\""));
                    assert!(!text.contains("\"path\""));
                    assert!(text.ends_with("--graphql--"));
                }
                SubscriptionTransport::WebSocket => unreachable!(),
            }
            for counter in [
                &probe.counts.factory_calls,
                &probe.counts.setup_calls,
                &probe.counts.source_polls,
                &probe.counts.hook_calls,
                &probe.counts.completion_calls,
            ] {
                assert_eq!(count(counter), 0);
            }
            assert_eq!(signals.completed().len(), 1);
            assert!(bytes.used() > 0);
            drop(response);
            drain(&runtime, guard);
            assert_eq!(bytes.used(), 0);
        }
    }
}

struct OversizedHook(Arc<State>);

impl ResponseHook<HookState> for OversizedHook {
    fn call<'a>(
        &'a self,
        _: ResponseHookContext<'a, HookState>,
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        self.0.counts.hook_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(count(&self.0.counts.setup_context_drops), 1);
        Box::pin(async {
            let mut headers = ResponseHeaders::new();
            headers.append("x-large", &"x".repeat(64 * 1024)).unwrap();
            Ok(headers)
        })
    }
}

#[test]
fn initial_profile_encoding_failure_retains_finite_fallback_metadata() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(Vec::new());
        let (mut endpoint, signals) = endpoint(Arc::clone(&probe));
        let state = Arc::get_mut(&mut endpoint).unwrap();
        let hook = crate::transport::erase_hook(OversizedHook(Arc::clone(&probe)));
        state.response_hook = Some(Arc::clone(&hook));
        Arc::get_mut(&mut state.subscription_execution)
            .unwrap()
            .response_hook = Some(hook);
        let bytes = state.subscriptions.resources.delivery().byte_owner();
        let guard = state
            .subscriptions
            .resources
            .register_worker_runtime()
            .unwrap();
        let pending = run(
            &runtime,
            pending(Arc::clone(&endpoint), profile, VALID, false),
        );
        let PreparedHttpSubscription::Response(response) = run(&runtime, pending.prepare()) else {
            panic!("bounded profile encoding failure must select a finite response");
        };
        assert_eq!(response.status(), 500);
        assert!(!response.is_streaming_profile());
        assert!(
            std::str::from_utf8(response.as_ref())
                .unwrap()
                .contains("INTERNAL_SERVER_ERROR")
        );
        assert!(response.headers().iter().any(|header| {
            header.name == "content-type"
                && header
                    .value
                    .starts_with("application/graphql-response+json")
        }));
        assert_eq!(count(&probe.counts.source_polls), 0);
        assert_eq!(count(&probe.counts.hook_calls), 1);
        assert_eq!(count(&probe.counts.completion_calls), 1);
        assert_eq!(
            *probe.completion_outcomes.lock().unwrap(),
            [ResponseCompletionOutcome::EncodingLimitExceeded]
        );
        assert_eq!(signals.completed().len(), 1);
        drop(response);
        drain(&runtime, guard);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(bytes.used(), 0);
    }
}
