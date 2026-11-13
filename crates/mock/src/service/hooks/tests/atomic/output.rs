//! Enforce encode limits inside HTTP scope and discard the candidate on failure.

use super::*;

struct EncodeDeadline(Arc<HttpScope>);

impl DeadlineFactory for EncodeDeadline {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(std::future::poll_fn(|_| {
            // Fire the deadline only after encoding reports success, forcing the final completion poll to recheck it.
            if self.0.encoded_for_test() {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        }))
    }
}

#[tokio::test]
async fn deadline_after_execution_prevents_encoding_and_committing_a_ready_mutation() {
    let _case = crate::vm_gate::test_case();
    let fixture = fixture(&[]);
    let scope = Arc::new(HttpScope::new("127.0.0.1:49123".parse().unwrap()));
    let deadlines = EncodeDeadline(scope.clone());
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let body =
        serde_json::to_vec(&json!({"query":"mutation { add(id:\"candidate\") { id } }"})).unwrap();
    let response = scope
        .run(
            fixture
                .endpoint
                .handle(transport, BufferedBody::new(body), &deadlines),
        )
        .await;
    assert!(scope.encoded_for_test());
    assert_failure_code(&response, "MOCK_EXECUTION_TIMEOUT");
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    let response = request(&fixture, "mutation { add(id:\"next\") { id } }").await;
    assert_eq!(response.status, 200);
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        1
    );
    fixture.stop().await;
}

#[tokio::test]
async fn direct_response_encoding_failure_rolls_back_the_complete_candidate() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = fixture(&[]);
    fixture.endpoint =
        crate::service::dispatch::limited_for_test(fixture.worker.client.clone(), 128);
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let query = format!(
        "mutation {{ add(id:\"{}\") {{ id name }} }}",
        "x".repeat(100)
    );
    let body = serde_json::to_vec(&json!({"query":query})).unwrap();
    let response = fixture
        .endpoint
        .handle(transport, BufferedBody::new(body), &Clock)
        .await;
    assert_failure_code(&response, "MOCK_EXECUTION_LIMIT_EXCEEDED");
    assert_eq!(
        fixture
            .worker
            .client
            .snapshot(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap()
            .size()
            .0,
        0
    );
    let response = request(&fixture, "mutation { add(id:\"next\") { id } }").await;
    assert_eq!(response.status, 200);
}
