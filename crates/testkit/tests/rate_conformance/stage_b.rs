use super::*;

#[tokio::test]
async fn rate_04_stage_b_timeout_drops_the_provider_and_applies_the_failure_policy() {
    let errors = RecordingErrors::default();
    let closed_drop = Arc::new(AtomicBool::new(false));
    let closed_calls = Arc::new(AtomicUsize::new(0));
    let handler = stage_b_handler(
        CountingLimiter::new(LimiterMode::Pending(Arc::clone(&closed_drop))),
        ProviderFailurePolicy::FailClosed,
        Arc::clone(&closed_calls),
        errors.clone(),
    );
    let response = call(&handler, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&body)
            .unwrap()
            .contains("RATE_LIMIT_UNAVAILABLE")
    );
    assert!(closed_drop.load(Ordering::SeqCst));
    assert_eq!(closed_calls.load(Ordering::SeqCst), 0);

    let open_drop = Arc::new(AtomicBool::new(false));
    let open_calls = Arc::new(AtomicUsize::new(0));
    let handler = stage_b_handler(
        CountingLimiter::new(LimiterMode::Pending(Arc::clone(&open_drop))),
        ProviderFailurePolicy::FailOpen,
        Arc::clone(&open_calls),
        errors.clone(),
    );
    assert_eq!(
        call(&handler, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::OK
    );
    assert!(open_drop.load(Ordering::SeqCst));
    assert_eq!(open_calls.load(Ordering::SeqCst), 1);

    let failed_calls = Arc::new(AtomicUsize::new(0));
    let failed_closed = stage_b_handler(
        CountingLimiter::new(LimiterMode::Fail),
        ProviderFailurePolicy::FailClosed,
        Arc::clone(&failed_calls),
        errors.clone(),
    );
    assert_eq!(
        call(&failed_closed, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let failed_open = stage_b_handler(
        CountingLimiter::new(LimiterMode::Fail),
        ProviderFailurePolicy::FailOpen,
        Arc::clone(&failed_calls),
        errors.clone(),
    );
    assert_eq!(
        call(&failed_open, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(failed_calls.load(Ordering::SeqCst), 1);
    assert_eq!(errors.0.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn stage_b_rejects_inconsistent_decisions_and_fails_open() {
    let errors = RecordingErrors::default();
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let handler = stage_b_handler(
        CountingLimiter::new(LimiterMode::MismatchedRevision),
        ProviderFailurePolicy::FailOpen,
        Arc::clone(&resolver_calls),
        errors.clone(),
    );
    let response = call(&handler, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);
    assert_eq!(errors.0.load(Ordering::SeqCst), 1);
}

fn stage_b_handler(
    limiter: CountingLimiter,
    failure_policy: ProviderFailurePolicy,
    resolver_calls: Arc<AtomicUsize>,
    errors: RecordingErrors,
) -> Endpoint<TestContext> {
    let plugin = OperationRateLimit::new(
        OperationKey,
        UnitFieldCost,
        timed(limiter, Duration::from_millis(5)),
        rate_policy(),
        failure_policy,
        OperationCost::new(10).unwrap(),
    )
    .unwrap();
    endpoint_builder(PhaseLog::default(), resolver_calls, false)
        .inject(|components| {
            components.rate_limit().stage_b_provider(plugin);
            components.observability().error_observer(errors);
            components.http().execution_timeout(Duration::from_secs(1));
        })
        .context_factory(Factory(PhaseLog::default()))
        .build()
        .unwrap()
}
