#[path = "lifecycle_support.rs"]
pub mod lifecycle_support;

#[path = "rate_conformance/stage_b.rs"]
mod stage_b;
#[path = "rate_conformance/transport.rs"]
mod transport;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode};
use chigiri::{
    Endpoint, ErrorEvent, ErrorObserver, GraphQlError, LifecyclePhase, OperationContext,
};
use chigiri_http::{
    AdmissionError, BoxFuture, ContextFactory, DeadlineFactory, PrepareError, PreparedContext,
    RequestContext, TransportAdmission, TransportRequest,
};
use chigiri_policy::{
    OperationQuotaExtractor, OperationRateLimit, ProviderFailurePolicy, StageARateLimit,
    TimedRateLimiter, TransportQuotaExtractor, UnitFieldCost,
};
use chigiri_rate::{
    MonotonicTick, OperationCost, PolicyRevision, QuotaKey, QuotaSubject, RateLimitDecision,
    RateLimitProviderError, RateLimitRejectReason, RateLimitRequest, RateLimiter, RatePolicy,
    TokenBucketPolicy,
};
use http_body_util::BodyExt as _;

use lifecycle_support::{PhaseLog, TestContext, endpoint_builder};

#[derive(Clone)]
enum LimiterMode {
    Allow,
    Reject,
    Fail,
    Pending(Arc<AtomicBool>),
    MismatchedLimit,
    MismatchedRevision,
}

#[derive(Clone)]
struct CountingLimiter {
    calls: Arc<AtomicUsize>,
    mode: LimiterMode,
}

#[derive(Clone, Default)]
struct RecordingErrors(Arc<AtomicUsize>);

impl ErrorObserver for RecordingErrors {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        assert_eq!(event.code(), "RATE_LIMIT_PROVIDER_FAILED");
        assert!(matches!(
            event.phase(),
            LifecyclePhase::TransportAdmission | LifecyclePhase::OperationAdmission
        ));
        assert!(matches!(
            event.component(),
            "stage-a-rate-limiter" | "stage-b-rate-limiter"
        ));
        assert!(!format!("{:?}", event.source()).contains("provider-secret"));
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl CountingLimiter {
    fn new(mode: LimiterMode) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            mode,
        }
    }
}

impl RateLimiter for CountingLimiter {
    fn check_and_consume<'a>(
        &'a self,
        request: RateLimitRequest<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<RateLimitDecision, RateLimitProviderError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.mode {
            LimiterMode::Pending(dropped) => Box::pin(PendingLimiter(Arc::clone(dropped))),
            mode => {
                let result = match mode {
                    LimiterMode::Allow => Ok(decision(request, None)),
                    LimiterMode::Reject => Ok(decision(request, Some(Duration::from_millis(1500)))),
                    LimiterMode::Fail => Err(RateLimitProviderError::new(std::io::Error::other(
                        "provider-secret",
                    ))),
                    LimiterMode::Pending(_) => unreachable!(),
                    LimiterMode::MismatchedLimit => Ok(mismatched_decision(request, false)),
                    LimiterMode::MismatchedRevision => Ok(mismatched_decision(request, true)),
                };
                Box::pin(async move { result })
            }
        }
    }
}

struct PendingLimiter(Arc<AtomicBool>);

impl Future for PendingLimiter {
    type Output = Result<RateLimitDecision, RateLimitProviderError>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for PendingLimiter {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct TokioDeadline;

impl DeadlineFactory for TokioDeadline {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

struct TransportKey;

impl TransportQuotaExtractor for TransportKey {
    fn extract(&self, _: TransportRequest<'_>) -> Result<QuotaKey, AdmissionError> {
        Ok(quota_key(b"transport"))
    }
}

struct OperationKey;

impl OperationQuotaExtractor<TestContext> for OperationKey {
    fn extract(&self, _: &OperationContext<'_, TestContext>) -> Result<QuotaKey, GraphQlError> {
        Ok(quota_key(b"operation"))
    }
}

struct Factory(PhaseLog);

impl ContextFactory<TestContext, ()> for Factory {
    fn create<'a>(
        &'a self,
        _: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<TestContext, ()>, PrepareError>> {
        self.0.push("context");
        let context = TestContext {
            log: self.0.clone(),
        };
        Box::pin(async move { Ok(PreparedContext::execution_only(context)) })
    }
}

#[tokio::test]
async fn rate_02_stage_b_rejection_does_not_roll_back_stage_a_and_skips_resolver() {
    let stage_a = CountingLimiter::new(LimiterMode::Allow);
    let stage_b = CountingLimiter::new(LimiterMode::Reject);
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let plugin = OperationRateLimit::new(
        OperationKey,
        UnitFieldCost,
        stage_b.clone(),
        rate_policy(),
        ProviderFailurePolicy::FailClosed,
        OperationCost::new(10).unwrap(),
    )
    .unwrap();
    let stage_a_admission = StageARateLimit::new(
        TransportKey,
        stage_a.clone(),
        rate_policy(),
        ProviderFailurePolicy::FailClosed,
    );
    let handler = endpoint_builder(PhaseLog::default(), Arc::clone(&resolver_calls), false)
        .inject(|components| {
            components
                .rate_limit()
                .stage_a_provider(stage_a_admission)
                .stage_b_provider(plugin);
        })
        .context_factory(Factory(PhaseLog::default()))
        .build()
        .unwrap();
    let response = call(&handler, post(Body::from(query_body()))).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(stage_a.calls.load(Ordering::SeqCst), 1);
    assert_eq!(stage_b.calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rate_04_timeout_drop_and_fail_open_closed() {
    let errors = RecordingErrors::default();
    let closed_drop = Arc::new(AtomicBool::new(false));
    let pending = StageARateLimit::new(
        TransportKey,
        timed(
            CountingLimiter::new(LimiterMode::Pending(Arc::clone(&closed_drop))),
            Duration::from_millis(5),
        ),
        rate_policy(),
        ProviderFailurePolicy::FailClosed,
    );
    let handler =
        handler_with_observed_stage(pending, Duration::from_secs(1), errors.clone(), true);
    assert_eq!(
        call(&handler, post(Body::from(query_body())))
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(closed_drop.load(Ordering::SeqCst));

    let open_drop = Arc::new(AtomicBool::new(false));
    let pending = StageARateLimit::new(
        TransportKey,
        timed(
            CountingLimiter::new(LimiterMode::Pending(Arc::clone(&open_drop))),
            Duration::from_millis(5),
        ),
        rate_policy(),
        ProviderFailurePolicy::FailOpen,
    );
    assert_eq!(
        call(
            &handler_with_observed_stage(pending, Duration::from_secs(1), errors.clone(), false,),
            post(Body::from(query_body())),
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert!(open_drop.load(Ordering::SeqCst));

    let closed = StageARateLimit::new(
        TransportKey,
        CountingLimiter::new(LimiterMode::Fail),
        rate_policy(),
        ProviderFailurePolicy::FailClosed,
    );
    assert_eq!(
        call(
            &handler_with_observed_stage(closed, Duration::from_secs(1), errors.clone(), false,),
            post(Body::from(query_body())),
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let open = StageARateLimit::new(
        TransportKey,
        CountingLimiter::new(LimiterMode::Fail),
        rate_policy(),
        ProviderFailurePolicy::FailOpen,
    );
    assert_eq!(
        call(
            &handler_with_observed_stage(open, Duration::from_secs(1), errors.clone(), true,),
            post(Body::from(query_body())),
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(errors.0.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn production_structural_limit_applies_without_a_rate_08_limiter() {
    let calls = Arc::new(AtomicUsize::new(0));
    let handler = endpoint_builder(PhaseLog::default(), Arc::clone(&calls), false)
        .context_factory(Factory(PhaseLog::default()))
        .build()
        .unwrap();
    let fields = (0..1001)
        .map(|index| format!("f{index}: hello"))
        .collect::<Vec<_>>()
        .join(" ");
    let body = format!(r#"{{"query":"{{ {fields} }}"}}"#);
    assert_eq!(
        call(&handler, post(Body::from(body))).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn handler_with_observed_stage(
    stage: impl TransportAdmission + chigiri::rate::RateLimitObserverBinding,
    timeout: Duration,
    observer: RecordingErrors,
    observer_first: bool,
) -> Endpoint<TestContext> {
    endpoint_builder(PhaseLog::default(), Arc::new(AtomicUsize::new(0)), false)
        .inject(|components| {
            if observer_first {
                components.observability().error_observer(observer);
                components.rate_limit().stage_a_provider(stage);
            } else {
                components.rate_limit().stage_a_provider(stage);
                components.observability().error_observer(observer);
            }
            components.http().transport_admission_timeout(timeout);
        })
        .context_factory(Factory(PhaseLog::default()))
        .build()
        .unwrap()
}

fn timed(
    limiter: CountingLimiter,
    timeout: Duration,
) -> TimedRateLimiter<CountingLimiter, TokioDeadline> {
    TimedRateLimiter::new(limiter, TokioDeadline, timeout).unwrap()
}

async fn call(endpoint: &Endpoint<TestContext>, request: Request) -> axum::response::Response {
    chigiri_axum::handler(endpoint.clone())(request).await
}

fn decision(request: RateLimitRequest<'_>, retry_after: Option<Duration>) -> RateLimitDecision {
    match retry_after {
        Some(retry_after) => RateLimitDecision::rejected(
            request,
            0,
            MonotonicTick::from_nanos(10),
            Some(retry_after),
            RateLimitRejectReason::QuotaExceeded,
        ),
        None => RateLimitDecision::allowed(request, 1, MonotonicTick::from_nanos(10)),
    }
    .unwrap()
}

fn mismatched_decision(request: RateLimitRequest<'_>, revision_only: bool) -> RateLimitDecision {
    let policy = TokenBucketPolicy::new(
        if revision_only { 10 } else { 11 },
        1,
        Duration::from_secs(1),
    )
    .unwrap()
    .with_revision(PolicyRevision::new(2).unwrap())
    .into();
    let wrong_request = RateLimitRequest::new(request.key(), request.cost(), policy);
    RateLimitDecision::allowed(wrong_request, 1, MonotonicTick::from_nanos(10)).unwrap()
}

fn quota_key(subject: &[u8]) -> QuotaKey {
    QuotaKey::new(
        b"test".to_vec(),
        QuotaSubject::value(subject.to_vec()).unwrap(),
        None,
    )
    .unwrap()
}

fn rate_policy() -> RatePolicy {
    TokenBucketPolicy::new(10, 1, Duration::from_secs(1))
        .unwrap()
        .into()
}

fn post(body: Body) -> Request {
    Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept", "application/graphql-response+json")
        .body(body)
        .unwrap()
}

const fn query_body() -> &'static str {
    r#"{"query":"{ hello }"}"#
}
