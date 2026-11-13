use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::task::{Context, Poll, Waker};

use chigiri_http::{
    AcceptConnectionInit, ApplicationMetadata, ConnectionInitAuthorizer, ConnectionInitContext,
    ConnectionInitDecision, ConnectionInitError, MetadataField, RequestContext, RequestHeader,
    RequestMethod, RequestPhase, ResponseMediaType, SubscriptionTransport, TransportRequest,
    WebSocketOneShot,
};
use chigiri_runtime::{GraphQlRequest, Object, Value};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn finite_is_the_default_and_keeps_the_existing_body_view() {
    let fixture = Fixture::new();
    let context = fixture.request_context().with_body();

    assert_eq!(context.phase(), RequestPhase::Finite);
    assert_eq!(RequestPhase::default(), RequestPhase::Finite);
    assert_eq!(context.raw_body(), b"finite-body");
    assert!(context.connection_init().is_none());

    let filtered = context.__with_phase(RequestPhase::Finite, Some(&fixture.payload));
    assert_eq!(filtered.raw_body(), b"finite-body");
    assert!(filtered.connection_init().is_none());
}

#[test]
fn only_websocket_phases_expose_initialization_data() {
    let fixture = Fixture::new();
    let init = Some(&fixture.payload);
    let base = fixture.request_context().with_body();

    let query = base.__with_phase(
        RequestPhase::WebSocketOperation {
            operation: WebSocketOneShot::Query,
        },
        init,
    );
    let mutation = base.__with_phase(
        RequestPhase::WebSocketOperation {
            operation: WebSocketOneShot::Mutation,
        },
        init,
    );
    let setup = base.__with_phase(
        RequestPhase::SubscriptionSetup {
            transport: SubscriptionTransport::WebSocket,
        },
        init,
    );
    let event = base.__with_phase(
        RequestPhase::SubscriptionEvent {
            transport: SubscriptionTransport::WebSocket,
            sequence: 7,
        },
        init,
    );

    for context in [query, mutation, setup, event] {
        assert!(std::ptr::eq(
            context.connection_init().unwrap(),
            &fixture.payload
        ));
        assert!(context.raw_body().is_empty());
    }
}

#[test]
fn http_streaming_phases_strip_body_and_websocket_credentials() {
    let fixture = Fixture::new();
    let base = fixture.request_context().with_body();
    let phases = [
        RequestPhase::SubscriptionSetup {
            transport: SubscriptionTransport::ServerSentEvents,
        },
        RequestPhase::SubscriptionEvent {
            transport: SubscriptionTransport::ServerSentEvents,
            sequence: 1,
        },
        RequestPhase::SubscriptionSetup {
            transport: SubscriptionTransport::Multipart,
        },
        RequestPhase::SubscriptionEvent {
            transport: SubscriptionTransport::Multipart,
            sequence: 2,
        },
    ];

    for phase in phases {
        let context = base
            .__with_phase(phase, Some(&fixture.payload))
            .__with_raw_body(b"must-not-return");
        assert_eq!(context.phase(), phase);
        assert!(context.raw_body().is_empty());
        assert!(context.connection_init().is_none());
    }
}

#[test]
fn connection_init_view_contains_only_admitted_borrowed_inputs() {
    let fixture = Fixture::new();
    let context = ConnectionInitContext::__new(fixture.transport(), Some(&fixture.payload));

    assert_eq!(context.headers(), fixture.headers.as_slice());
    assert_eq!(
        context.application_metadata(),
        &fixture.application_metadata
    );
    assert_eq!(context.peer_addr(), Some(fixture.peer));
    assert!(std::ptr::eq(context.payload().unwrap(), &fixture.payload));

    let debug = format!("{context:?}");
    assert!(!debug.contains("secret"));
    assert_send_sync::<ConnectionInitContext<'static>>();
}

#[test]
fn default_authorizer_accepts_through_the_object_safe_spi() {
    let fixture = Fixture::new();
    let context = ConnectionInitContext::__new(fixture.transport(), Some(&fixture.payload));
    let policy: &dyn ConnectionInitAuthorizer = &AcceptConnectionInit;

    assert_eq!(
        ready(policy.authorize(context)).unwrap(),
        ConnectionInitDecision::Accept
    );
    assert_send_sync::<AcceptConnectionInit>();
}

#[test]
fn connection_init_error_redacts_its_internal_source() {
    let error = ConnectionInitError::new(SecretError);

    assert!(!format!("{error:?}").contains("secret"));
    assert!(!error.to_string().contains("secret"));
    assert!(error.source().is_some());
    assert_send_sync::<ConnectionInitError>();
}

fn ready<T>(mut future: chigiri_http::BoxFuture<'_, T>) -> T {
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test future unexpectedly remained pending"),
    }
}

trait RequestContextTestExt<'a> {
    fn with_body(self) -> RequestContext<'a>;
}

impl<'a> RequestContextTestExt<'a> for RequestContext<'a> {
    fn with_body(self) -> RequestContext<'a> {
        self.__with_raw_body(b"finite-body")
    }
}

struct Fixture {
    request: GraphQlRequest,
    headers: Vec<RequestHeader<'static>>,
    application_metadata: ApplicationMetadata,
    peer: SocketAddr,
    payload: Object,
}

impl Fixture {
    fn new() -> Self {
        let mut payload = Object::new();
        payload.insert("credential", Value::String("secret-payload".to_owned()));
        Self {
            request: GraphQlRequest::new("query Q { __typename }"),
            headers: vec![RequestHeader::new("authorization", b"secret-header")],
            application_metadata: ApplicationMetadata::new(vec![
                MetadataField::new("tenant", b"secret-metadata".to_vec()).unwrap(),
            ])
            .unwrap(),
            peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080),
            payload,
        }
    }

    fn transport(&self) -> TransportRequest<'_> {
        TransportRequest::new(
            "POST",
            "/graphql",
            None,
            &self.headers,
            Some(self.peer),
            &self.application_metadata,
        )
    }

    fn request_context(&self) -> RequestContext<'_> {
        RequestContext::__new(
            self.transport(),
            &self.request,
            RequestMethod::Post,
            ResponseMediaType::GraphQlResponseJson,
        )
    }
}

#[derive(Debug)]
struct SecretError;

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("secret-internal-source")
    }
}

impl Error for SecretError {}
