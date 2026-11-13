//! Built graph ownership and configured authorization integration proof.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, ConnectionInitAuthorizer, ConnectionInitContext,
    ConnectionInitDecision, ConnectionInitError, ContextFactory, PrepareError, PreparedContext,
    RequestContext, RequestHeader, TransportRequest,
};

use crate::__streaming::BridgeOperationKind;
use crate::subscription::delivery::ByteReservationError;
use crate::subscription::request::test_support::schema;
use crate::{Endpoint, SubscriptionLimits, SubscriptionProtocolSettings};

fn limits() -> SubscriptionLimits {
    SubscriptionLimits {
        max_active_subscriptions: 1,
        max_queued_events: 2,
        max_queued_bytes: 128,
        max_encoded_event_bytes: 64,
        max_endpoint_encoded_bytes: 128,
        max_pending_operation_setups: 1,
        max_retained_metadata_bytes: 123,
        setup_timeout: Duration::from_millis(234),
        event_timeout: Duration::from_millis(345),
        ..SubscriptionLimits::default()
    }
}

fn protocol() -> SubscriptionProtocolSettings {
    SubscriptionProtocolSettings {
        max_active_operations_per_connection: 1,
        max_websocket_connections: 1,
        max_pending_setups_per_connection: 1,
    }
}

fn endpoint() -> Endpoint<()> {
    Endpoint::builder(schema())
        .subscription_limits(limits())
        .subscription_protocol(protocol())
        .without_context()
        .build()
        .unwrap()
}

#[test]
fn built_driver_uses_the_exact_validated_effective_graph() {
    let endpoint = endpoint();
    let state = endpoint.neutral.shared_state();
    assert_eq!(state.subscriptions.settings.limits(), &limits());
    assert_eq!(state.subscriptions.settings.protocol(), &protocol());
    assert_eq!(
        state.subscriptions.settings.limits(),
        endpoint.configuration().subscription_limits().effective()
    );
    assert_eq!(
        state.subscriptions.settings.protocol(),
        endpoint.configuration().subscription_protocol().effective()
    );
    assert_eq!(
        state
            .subscriptions
            .resources
            .delivery()
            .byte_owner()
            .limit(),
        128
    );
}

#[test]
fn clones_share_physical_quotas_and_bytes_but_independent_builds_do_not() {
    let endpoint = endpoint();
    let clone = endpoint.clone();
    let independent = self::endpoint();
    let state = endpoint.neutral.shared_state();
    let cloned = clone.neutral.shared_state();
    let separate = independent.neutral.shared_state();
    assert!(Arc::ptr_eq(&state, &cloned));
    assert!(!Arc::ptr_eq(&state, &separate));

    let pending = endpoint.__begin_http_subscription().unwrap();
    assert!(clone.__begin_http_subscription().is_err());
    let separate_pending = independent.__begin_http_subscription().unwrap();
    let active = pending.activate(BridgeOperationKind::Subscription).unwrap();
    let second_pending = clone.__begin_http_subscription().unwrap();
    assert!(
        second_pending
            .activate(BridgeOperationKind::Subscription)
            .is_err()
    );
    let resources = state.subscriptions.resources.delivery();
    assert_eq!(resources.snapshot().endpoint_pending, 0);
    assert_eq!(resources.snapshot().endpoint_active_subscriptions, 1);

    let connection = endpoint.__open_subscription_connection().unwrap();
    assert!(clone.__open_subscription_connection().is_err());
    let separate_connection = independent.__open_subscription_connection().unwrap();
    let byte_owner = resources.byte_owner();
    let reservation = byte_owner.try_reserve(128).unwrap();
    assert!(matches!(
        cloned
            .subscriptions
            .resources
            .delivery()
            .byte_owner()
            .try_reserve(1),
        Err(ByteReservationError::Capacity)
    ));
    let separate_reservation = separate
        .subscriptions
        .resources
        .delivery()
        .byte_owner()
        .try_reserve(128)
        .unwrap();
    drop((reservation, active, connection));
    assert_eq!(byte_owner.used(), 0);
    let snapshot = cloned.subscriptions.resources.delivery().snapshot();
    assert_eq!(snapshot.endpoint_active_subscriptions, 0);
    assert_eq!(snapshot.endpoint_connections, 0);
    assert!(!snapshot.corrupted);
    drop((separate_pending, separate_connection, separate_reservation));
}

struct MustNotPrepare;

impl ContextFactory<(), ()> for MustNotPrepare {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<(), ()>, PrepareError>> {
        panic!("connection authorization must not prepare an operation context")
    }
}

struct RejectInitialization(Arc<AtomicUsize>);

impl ConnectionInitAuthorizer for RejectInitialization {
    fn authorize<'a>(
        &'a self,
        request: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        assert_eq!(request.headers()[0].value, b"private-credential");
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ConnectionInitDecision::Reject) })
    }
}

#[test]
fn built_authorizer_uses_the_final_common_leaf_without_preparing_context() {
    let calls = Arc::new(AtomicUsize::new(0));
    let endpoint = Endpoint::builder(schema())
        .connection_init_authorizer(chigiri_http::AcceptConnectionInit)
        .inject(|components| {
            components
                .subscriptions()
                .connection_init_authorizer(RejectInitialization(Arc::clone(&calls)));
        })
        .context_factory(MustNotPrepare)
        .build()
        .unwrap();
    let clone = endpoint.clone();
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("authorization", b"private-credential")];
    let transport = TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata);
    for owner in [&endpoint, &clone] {
        let decision =
            ready(owner.__authorize_connection_init(ConnectionInitContext::__new(transport, None)))
                .unwrap();
        assert_eq!(decision, ConnectionInitDecision::Reject);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn built_default_authorizer_accepts() {
    let endpoint = endpoint();
    let metadata = ApplicationMetadata::default();
    let transport = TransportRequest::new("GET", "/graphql", None, &[], None, &metadata);
    assert_eq!(
        ready(endpoint.__authorize_connection_init(ConnectionInitContext::__new(transport, None)))
            .unwrap(),
        ConnectionInitDecision::Accept
    );
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("the test authorization callback must complete immediately"),
    }
}
