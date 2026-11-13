//! Fix the stable identity, drop, tie order of direct/HTTP timeout.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri::spi::{ApplicationMetadata, BufferedBody, RequestHeader};

use super::support::{block_on, resolvers};

#[test]
fn direct_and_http_return_the_same_request_timeout_and_drop_the_target_future() {
    let direct_drop = Arc::new(AtomicBool::new(false));
    let direct = endpoint(Arc::clone(&direct_drop));
    let response = block_on(direct.execute(
        chigiri::GraphQlRequest::new("{ greeting }"),
        (),
        &ExecutionDeadline,
    ));
    assert_timeout(&response);
    assert!(direct_drop.load(Ordering::SeqCst));

    let http_drop = Arc::new(AtomicBool::new(false));
    let http = endpoint(Arc::clone(&http_drop));
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let metadata = ApplicationMetadata::default();
    let request =
        chigiri::TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let response = block_on(http.handle(
        request,
        BufferedBody::new(br#"{"query":"{ greeting }"}"#.to_vec()),
        &ExecutionDeadline,
    ));
    assert_eq!(response.status, 504);
    let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(value["errors"][0]["message"], "Request timed out");
    assert_eq!(value["errors"][0]["extensions"]["code"], "REQUEST_TIMEOUT");
    assert!(http_drop.load(Ordering::SeqCst));
}

#[test]
fn immediate_success_wins_when_ready_at_the_timeout_boundary() {
    let endpoint = chigiri::Endpoint::builder(resolvers())
        .without_context()
        .build()
        .unwrap();
    let response = block_on(endpoint.execute(
        chigiri::GraphQlRequest::new("{ greeting }"),
        (),
        &ImmediateDeadline,
    ));
    assert_timeout(&response);
}

fn endpoint(dropped: Arc<AtomicBool>) -> chigiri::Endpoint<()> {
    chigiri::Endpoint::builder(resolvers())
        .push_plugin(PendingRequest(dropped))
        .execution_timeout(Duration::from_secs(1))
        .without_context()
        .build()
        .unwrap()
}

fn assert_timeout(response: &chigiri::GraphQlResponse) {
    assert!(response.data.is_none());
    assert_eq!(response.errors[0].message, "Request timed out");
    assert_eq!(
        response.errors[0]
            .extensions
            .get("code")
            .and_then(chigiri::Value::as_str),
        Some("REQUEST_TIMEOUT")
    );
}

struct PendingRequest(Arc<AtomicBool>);

impl chigiri::Plugin<()> for PendingRequest {
    fn request<'a>(
        &'a self,
        _: chigiri::RequestContext<'a, ()>,
        _: chigiri::RequestNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        Box::pin(PendingResponse(Arc::clone(&self.0)))
    }
}

struct PendingResponse(Arc<AtomicBool>);

impl Future for PendingResponse {
    type Output = chigiri::GraphQlResponse;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingResponse {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct ExecutionDeadline;

impl chigiri::DeadlineFactory for ExecutionDeadline {
    fn sleep(&self, duration: Duration) -> chigiri::BoxFuture<'_, ()> {
        if duration == Duration::from_secs(1) {
            Box::pin(SecondPoll(false))
        } else {
            Box::pin(std::future::pending())
        }
    }
}

struct SecondPoll(bool);

impl Future for SecondPoll {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, task: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            task.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

struct ImmediateDeadline;

impl chigiri::DeadlineFactory for ImmediateDeadline {
    fn sleep(&self, _: Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}
