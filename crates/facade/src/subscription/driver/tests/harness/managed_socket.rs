//! Actual SDK-managed upgrade/session with a controllable protocol peer.

use std::time::Duration;

use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader, TransportRequest,
    WebSocketHttpVersion,
};

use super::super::state::AppContext;
use super::super::websocket_session::socket::Handle;
use crate::Endpoint;
use crate::managed::{ManagedHttpResponse, ManagedService, ManagedWebSocketUpgrade};

struct Timers;
static TIMERS: Timers = Timers;
impl DeadlineFactory for Timers {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

pub(in crate::subscription::driver::tests) struct ManagedSocket {
    pub(in crate::subscription::driver::tests) service: ManagedService,
    pub(in crate::subscription::driver::tests) peer: Handle,
}

pub(in crate::subscription::driver::tests) async fn open(
    endpoint: &Endpoint<AppContext>,
) -> ManagedSocket {
    let managed = begin(endpoint).await;
    let peer = &managed.peer;
    tokio::time::timeout(Duration::from_secs(2), async {
        while peer.messages().is_empty() || peer.has_current_frame() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native peer waits for the flushed connection acknowledgement");
    assert!(
        peer.messages()
            .iter()
            .any(|frame| frame == r#"{"type":"connection_ack"}"#)
    );
    managed
}

pub(in crate::subscription::driver::tests) async fn begin(
    endpoint: &Endpoint<AppContext>,
) -> ManagedSocket {
    let (service, upgrade) = admit(endpoint).await;
    let peer = Handle::default();
    peer.text(r#"{"type":"connection_init"}"#);
    let native = peer.clone();
    endpoint
        .__attach_managed_websocket(
            upgrade,
            Box::pin(async move { Ok(native.socket()) }),
            &TIMERS,
        )
        .unwrap();
    ManagedSocket { service, peer }
}

pub(in crate::subscription::driver::tests) async fn admit(
    endpoint: &Endpoint<AppContext>,
) -> (ManagedService, ManagedWebSocketUpgrade) {
    let service = endpoint.__managed_service().unwrap();
    let headers = [
        RequestHeader::new("host", b"example.test"),
        RequestHeader::new("connection", b"Upgrade"),
        RequestHeader::new("upgrade", b"websocket"),
        RequestHeader::new("sec-websocket-version", b"13"),
        RequestHeader::new("sec-websocket-key", b"dGhlIHNhbXBsZSBub25jZQ=="),
        RequestHeader::new("sec-websocket-protocol", b"graphql-transport-ws"),
        RequestHeader::new("x-chigiri-preflight", b"1"),
        RequestHeader::new("authorization", b"original-secret"),
    ];
    let metadata = ApplicationMetadata::default();
    let upgrade = match endpoint
        .__handle_managed_http(
            service.request().unwrap(),
            TransportRequest::new("GET", "/graphql", None, &headers, None, &metadata),
            BufferedBody::new(Vec::new()),
            WebSocketHttpVersion::Http11,
            &TIMERS,
        )
        .await
    {
        ManagedHttpResponse::Upgrade(upgrade) => *upgrade,
        _ => panic!("managed opening must admit the fixture upgrade"),
    };
    (service, upgrade)
}
