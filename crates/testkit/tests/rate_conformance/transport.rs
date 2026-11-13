//! Rate-contract checks for transport body polling and redaction.

use chigiri_http::{ApplicationMetadata, RequestHeader};
use futures_util::Stream;

use super::*;

#[tokio::test]
async fn rate_01_and_05_stage_a_returns_429_with_retry_after_before_reading_the_body() {
    let polls = Arc::new(AtomicUsize::new(0));
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let limiter = CountingLimiter::new(LimiterMode::Reject);
    let stage = StageARateLimit::new(
        TransportKey,
        limiter,
        rate_policy(),
        ProviderFailurePolicy::FailClosed,
    );
    let handler = endpoint_builder(PhaseLog::default(), Arc::clone(&resolver_calls), false)
        .inject(|components| {
            components.rate_limit().stage_a_provider(stage);
        })
        .context_factory(Factory(PhaseLog::default()))
        .build()
        .unwrap();
    let response = call(
        &handler,
        post(Body::from_stream(PollStream(Arc::clone(&polls)))),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()["retry-after"], "2");
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["errors"][0]["extensions"]["code"], "RATE_LIMITED");
    assert_eq!(json["errors"][0]["extensions"]["retryAfterSeconds"], 2);
}

#[tokio::test]
async fn inconsistent_decisions_are_bounded_and_follow_fail_open_configuration() {
    let errors = RecordingErrors::default();
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let stage = StageARateLimit::new(
        TransportKey,
        CountingLimiter::new(LimiterMode::MismatchedLimit),
        rate_policy(),
        ProviderFailurePolicy::FailOpen,
    );
    let handler = endpoint_builder(PhaseLog::default(), Arc::clone(&resolver_calls), false)
        .inject(|components| {
            components.rate_limit().stage_a_provider(stage);
            components.observability().error_observer(errors.clone());
        })
        .context_factory(Factory(PhaseLog::default()))
        .build()
        .unwrap();
    let response = call(&handler, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);
    assert_eq!(errors.0.load(Ordering::SeqCst), 1);
}

struct PollStream(Arc<AtomicUsize>);

impl Stream for PollStream {
    type Item = Result<bytes::Bytes, std::io::Error>;
    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Poll::Pending
    }
}

#[test]
fn rate_06_transport_debug_does_not_display_token_or_quota_value() {
    let headers = [RequestHeader::new("authorization", b"secret-token")];
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    assert!(!format!("{request:?}").contains("secret-token"));
    assert!(!format!("{:?}", quota_key(b"tenant-secret")).contains("tenant-secret"));
}
