use std::sync::Arc;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::LifecycleResult;

use super::super::state::{SourceStep, State, count};
use super::support::{VALID, drain, endpoint, pending, run, runtime};
use crate::transport::streaming::{
    HttpSubscriptionEvents, HttpSubscriptionPreparation, PreparedHttpSubscription,
    SubscriptionInitialResponse,
};

#[test]
fn joined_active_capacity_rejection_preserves_the_healthy_source_and_records_rejected() {
    let runtime = runtime();
    let probe = State::new(vec![SourceStep::Pending]);
    let (endpoint, signals) = endpoint(Arc::clone(&probe));
    let guard = endpoint
        .subscriptions
        .resources
        .register_worker_runtime()
        .unwrap();
    let first = run(
        &runtime,
        pending(
            Arc::clone(&endpoint),
            SubscriptionTransport::ServerSentEvents,
            VALID,
            false,
        ),
    );
    let PreparedHttpSubscription::Ready(healthy) = run(&runtime, first.prepare()) else {
        panic!("first source must hold the sole active permit");
    };
    let second = run(
        &runtime,
        pending(
            Arc::clone(&endpoint),
            SubscriptionTransport::Multipart,
            VALID,
            false,
        ),
    );
    let PreparedHttpSubscription::Response(response) = run(&runtime, second.prepare()) else {
        panic!("active capacity must reject without eviction");
    };
    assert_eq!(response.status(), 503);
    assert!(
        std::str::from_utf8(response.as_ref())
            .unwrap()
            .contains("SUBSCRIPTION_LIMIT_EXCEEDED")
    );
    assert_eq!(count(&probe.counts.factory_calls), 1);
    assert_eq!(count(&probe.counts.setup_calls), 1);
    assert_eq!(count(&probe.counts.hook_calls), 1);
    assert_eq!(count(&probe.counts.completion_calls), 1);
    assert_eq!(count(&probe.counts.source_polls), 0);
    assert_eq!(count(&probe.counts.source_drops), 0);
    assert_eq!(
        signals.completed(),
        [
            (LifecycleResult::Succeeded, None),
            (
                LifecycleResult::Rejected,
                Some("SUBSCRIPTION_LIMIT_EXCEEDED")
            ),
        ]
    );
    drop((response, healthy));
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

#[test]
fn public_hidden_owners_are_send_static_and_not_clone() {
    fn assert_send<T: Send + 'static>() {}
    fn assert_bytes<T: AsRef<[u8]> + Send + Sync + 'static>() {}
    assert_send::<HttpSubscriptionPreparation>();
    assert_send::<HttpSubscriptionEvents>();
    assert_bytes::<SubscriptionInitialResponse>();
    let _ = <SubscriptionInitialResponse as AmbiguousIfClone<_>>::marker;
    let _ = <HttpSubscriptionPreparation as AmbiguousIfClone<_>>::marker;
    let _ = <HttpSubscriptionEvents as AmbiguousIfClone<_>>::marker;
}

trait AmbiguousIfClone<Marker> {
    fn marker() {}
}
impl<T: ?Sized> AmbiguousIfClone<()> for T {}
struct Invalid;
impl<T: Clone> AmbiguousIfClone<Invalid> for T {}
