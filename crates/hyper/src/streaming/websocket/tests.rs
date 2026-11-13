//! Loopback proof for the explicit Hyper upgrade boundary.

mod capacity;
mod termination;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use chigiri::core::Source;
use chigiri::core::executable::CustomScalarRegistry;
use chigiri::core::executor::{ArgumentValues, ResolvedObject, ResolvedValue};
use chigiri::experimental::{OwnedResolverDispatch, OwnedSchema};
use chigiri::{BoxFuture, Endpoint, FieldResult, SubscriptionProtocolSettings, Value};
use chigiri_http::{
    AdmissionDecision, AdmissionError, ConnectionInitAuthorizer, ConnectionInitContext,
    ConnectionInitDecision, ConnectionInitError, TransportAdmission, TransportRequest,
};
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;

use crate::streaming::streaming_service;

struct Dispatch {
    schema: OwnedSchema,
    coordinates: [(Arc<str>, Arc<str>); 1],
}

#[derive(Default)]
struct Signals {
    admission: AtomicUsize,
    authorizer: AtomicUsize,
}

struct Admission(Arc<Signals>);

impl TransportAdmission for Admission {
    fn admit<'a>(
        &'a self,
        _: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.0.admission.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(AdmissionDecision::Continue) })
    }
}

struct Authorizer(Arc<Signals>);

impl ConnectionInitAuthorizer for Authorizer {
    fn authorize<'a>(
        &'a self,
        _: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        self.0.authorizer.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ConnectionInitDecision::Accept) })
    }
}

impl OwnedResolverDispatch<()> for Dispatch {
    fn schema(&self) -> &OwnedSchema {
        &self.schema
    }

    fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.coordinates
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async { Ok(ResolvedValue::Leaf(Value::Int(1))) })
    }
}

pub(super) fn endpoint() -> Endpoint<()> {
    endpoint_with_signals().0
}

fn endpoint_with_signals() -> (Endpoint<()>, Arc<Signals>) {
    let schema = OwnedSchema::parse(vec![Source::new(
        "hyper-websocket.graphql",
        "type Query { value: Int! }",
    )])
    .unwrap();
    let dispatch = Dispatch {
        schema: schema.clone(),
        coordinates: [(Arc::from("Query"), Arc::from("value"))],
    };
    let signals = Arc::new(Signals::default());
    let endpoint = Endpoint::builder(schema.bind(dispatch).unwrap())
        .without_context()
        .connection_init_authorizer(Authorizer(Arc::clone(&signals)))
        .subscription_protocol(SubscriptionProtocolSettings {
            max_websocket_connections: 1,
            ..SubscriptionProtocolSettings::default()
        })
        .inject(|components| {
            components
                .http()
                .push_transport_admission(Admission(Arc::clone(&signals)));
        })
        .build()
        .unwrap();
    (endpoint, signals)
}

#[tokio::test]
async fn actual_http11_upgrade_does_not_wait_for_the_declared_request_body() {
    let (response, signals) = serve(REQUEST).await;
    assert_switching_response(&response);
    assert_eq!(signals.admission.load(Ordering::SeqCst), 1);
    assert_eq!(signals.authorizer.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn shared_preflight_rejects_before_101_and_leaves_the_driver_idle() {
    let (response, signals) = serve(BAD_PROTOCOL_REQUEST).await;
    let response = std::str::from_utf8(&response).unwrap();
    assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
    assert!(!response.contains("sec-websocket-accept"));
    assert!(!response.contains("101 Switching Protocols"));
    assert_eq!(signals.admission.load(Ordering::SeqCst), 0);
    assert_eq!(signals.authorizer.load(Ordering::SeqCst), 0);
}

async fn serve(request: &'static [u8]) -> (Vec<u8>, Arc<Signals>) {
    let (endpoint, signals) = endpoint_with_signals();
    let (service, http_driver, guard) = streaming_service(endpoint).unwrap();
    let (upgrade_service, websocket_driver) = service.websocket_connection();
    drop(service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = tokio::task::spawn_blocking(move || exchange(address, request));
    let (stream, _) = listener.accept().await.unwrap();
    let connection = http1::Builder::new()
        .serve_connection(TokioIo::new(stream), upgrade_service)
        .with_upgrades();
    let (connection, websocket, response) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(connection, websocket_driver, client)
    })
    .await
    .expect("loopback upgrade must drain");
    connection.unwrap();
    websocket.unwrap();
    let response = response.unwrap();
    http_driver.await.unwrap();
    guard.shutdown().unwrap().await.unwrap();
    (response, signals)
}

fn exchange(address: SocketAddr, request: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(request).unwrap();
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        assert_eq!(stream.read(&mut byte).unwrap(), 1);
        response.push(byte[0]);
    }
    response
}

pub(super) fn assert_switching_response(response: &[u8]) {
    let response = std::str::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
    assert!(response.contains("connection: upgrade\r\n"));
    assert!(response.contains("upgrade: websocket\r\n"));
    assert!(response.contains("sec-websocket-protocol: graphql-transport-ws\r\n"));
    assert!(response.contains("sec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
    assert!(!response.to_ascii_lowercase().contains("content-length"));
}

pub(super) const REQUEST: &[u8] = b"GET /graphql HTTP/1.1\r\n\
Host: example.test\r\n\
Connection: Upgrade\r\n\
Upgrade: websocket\r\n\
Sec-WebSocket-Version: 13\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Protocol: graphql-transport-ws\r\n\
Content-Length: 100\r\n\
X-Chigiri-Preflight: 1\r\n\
\r\n";

const BAD_PROTOCOL_REQUEST: &[u8] = b"GET /graphql HTTP/1.1\r\n\
Host: example.test\r\n\
Connection: close, Upgrade\r\n\
Upgrade: websocket\r\n\
Sec-WebSocket-Version: 13\r\n\
Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Protocol: graphql-ws\r\n\
X-Chigiri-Preflight: 1\r\n\
\r\n";
