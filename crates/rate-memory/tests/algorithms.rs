use std::sync::Arc;
use std::time::Duration;

use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, MonotonicTick, OperationCost, QuotaKey, QuotaSubject,
    RateLimitDecision, RateLimitFuture, RateLimitRejectReason, RateLimitRequest, RateLimitSnapshot,
    RateLimiter, RatePolicy, SlidingWindowPolicy, TokenBucketPolicy,
};
use chigiri_rate_memory::{
    InMemoryFixedWindow, InMemoryLeakyBucket, InMemoryRateLimitConfig,
    InMemorySlidingWindowCounter, InMemoryTokenBucket, ManualClock,
};

mod limiters {
    use super::*;

    pub mod token_bucket {
        use super::*;

        #[test]
        fn limit_01_checks_boundaries_retry_delays_and_excess_costs() {
            let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(0)));
            let limiter = InMemoryTokenBucket::with_clock(
                config(4, Duration::from_nanos(100)),
                Arc::clone(&clock),
            );
            let policy = token_policy(2);
            let subject = key(b"token");
            assert_allowed(decide(&limiter, &subject, 2, policy));
            clock.set(MonotonicTick::from_nanos(9));
            assert_eq!(retry(decide(&limiter, &subject, 1, policy)), 1);
            assert_eq!(
                reject_reason(decide(&limiter, &subject, 3, policy)),
                RateLimitRejectReason::DynamicCostExceedsLimit
            );
            clock.set(MonotonicTick::from_nanos(10));
            assert_allowed(decide(&limiter, &subject, 1, policy));
        }
    }

    pub mod fixed_window {
        use super::*;

        #[test]
        fn limit_02_checks_window_boundaries_and_concurrent_consumption() {
            let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(9)));
            let limiter = InMemoryFixedWindow::with_clock(
                config(4, Duration::from_nanos(100)),
                Arc::clone(&clock),
            );
            let policy: RatePolicy = FixedWindowPolicy::new(2, Duration::from_nanos(10))
                .unwrap()
                .into();
            let subject = key(b"fixed");
            assert_allowed(decide(&limiter, &subject, 2, policy));
            assert_eq!(retry(decide(&limiter, &subject, 1, policy)), 1);
            clock.set(MonotonicTick::from_nanos(10));
            assert_allowed(decide(&limiter, &subject, 2, policy));

            let concurrent = Arc::new(InMemoryFixedWindow::with_clock(
                config(4, Duration::from_secs(60)),
                clock,
            ));
            let concurrent_policy: RatePolicy = FixedWindowPolicy::new(8, Duration::from_secs(60))
                .unwrap()
                .into();
            let handles = (0..32)
                .map(|_| {
                    let limiter = Arc::clone(&concurrent);
                    std::thread::spawn(move || {
                        decide(limiter.as_ref(), &key(b"same"), 1, concurrent_policy)
                    })
                })
                .collect::<Vec<_>>();
            let allowed = handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .filter(|decision| decision.is_allowed())
                .count();
            assert_eq!(allowed, 8);
        }
    }

    pub mod sliding_counter {
        use super::*;

        #[test]
        fn limit_03_checks_integer_rotation_overflow_and_retry_delays() {
            let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(0)));
            let limiter = InMemorySlidingWindowCounter::with_clock(
                config(4, Duration::from_nanos(1_000)),
                Arc::clone(&clock),
            );
            let policy: RatePolicy = SlidingWindowPolicy::new(10, Duration::from_nanos(100))
                .unwrap()
                .into();
            let subject = key(b"sliding");
            assert_allowed(decide(&limiter, &subject, 10, policy));
            clock.set(MonotonicTick::from_nanos(100));
            assert_eq!(retry(decide(&limiter, &subject, 1, policy)), 10);
            clock.set(MonotonicTick::from_nanos(109));
            assert_eq!(retry(decide(&limiter, &subject, 1, policy)), 1);
            clock.set(MonotonicTick::from_nanos(110));
            assert_allowed(decide(&limiter, &subject, 1, policy));

            let extreme = InMemorySlidingWindowCounter::with_clock(
                config(1, Duration::from_nanos(1)),
                ManualClock::new(MonotonicTick::from_nanos(0)),
            );
            let extreme_policy: RatePolicy =
                SlidingWindowPolicy::new(u64::MAX, Duration::from_nanos(u64::MAX))
                    .unwrap()
                    .into();
            let extreme_key = key(b"overflow");
            assert_allowed(decide(&extreme, &extreme_key, u64::MAX, extreme_policy));
            assert_eq!(
                reject_reason(decide(&extreme, &extreme_key, 1, extreme_policy)),
                RateLimitRejectReason::ArithmeticOverflow
            );
        }
    }

    pub mod leaky_meter {
        use super::*;

        #[test]
        fn limit_04_drains_without_allocating_queues_or_spawning_tasks() {
            let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(0)));
            let limiter = InMemoryLeakyBucket::with_clock(
                config(4, Duration::from_nanos(100)),
                Arc::clone(&clock),
            );
            let policy: RatePolicy = LeakyBucketPolicy::new(10, 2, Duration::from_nanos(10))
                .unwrap()
                .into();
            let subject = key(b"leaky");
            assert_allowed(decide(&limiter, &subject, 10, policy));
            clock.set(MonotonicTick::from_nanos(9));
            assert_eq!(retry(decide(&limiter, &subject, 1, policy)), 1);
            clock.set(MonotonicTick::from_nanos(10));
            assert_allowed(decide(&limiter, &subject, 2, policy));

            let source = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/leaky_bucket.rs"),
            )
            .unwrap();
            assert!(!source.contains("VecDeque"));
            assert!(!source.contains("spawn("));
            assert!(!source.contains("sleep("));
        }
    }
}

#[path = "algorithms/store_clock.rs"]
mod store_clock;

#[test]
fn quota_key_debug_output_redacts_secrets() {
    let key = key(b"tenant-secret-token");
    assert!(!format!("{key:?}").contains("tenant-secret-token"));
}

fn bounded_store(
    limiter: &impl RateLimiter,
    clock: &ManualClock,
    policy: RatePolicy,
    prefix: &[u8],
) {
    let first = key(&[prefix, b"-first"].concat());
    let second = key(&[prefix, b"-second"].concat());
    assert_allowed(decide(limiter, &first, 1, policy));
    assert_eq!(
        reject_reason(decide(limiter, &second, 1, policy)),
        RateLimitRejectReason::KeyCapacityExhausted
    );
    clock.advance(11);
    assert_allowed(decide(limiter, &second, 1, policy));
}

fn active_reject(
    limiter: &impl RateLimiter,
    clock: &ManualClock,
    policy: RatePolicy,
    prefix: &[u8],
) {
    let active = key(&[prefix, b"-active"].concat());
    let waiting = key(&[prefix, b"-waiting"].concat());
    let before = snapshot(decide(limiter, &active, 1, policy));
    clock.set(MonotonicTick::from_nanos(9));
    let rejected = snapshot(decide(limiter, &active, 1, policy));
    assert_eq!(
        rejected.reject_reason(),
        Some(RateLimitRejectReason::QuotaExceeded)
    );
    assert_eq!(
        (rejected.limit(), rejected.remaining(), rejected.revision()),
        (before.limit(), before.remaining(), before.revision())
    );
    clock.set(MonotonicTick::from_nanos(11));
    assert_eq!(
        reject_reason(decide(limiter, &waiting, 1, policy)),
        RateLimitRejectReason::KeyCapacityExhausted
    );
    clock.set(MonotonicTick::from_nanos(20));
    assert_allowed(decide(limiter, &waiting, 1, policy));
}

fn key(value: &[u8]) -> QuotaKey {
    QuotaKey::new(
        b"rate".to_vec(),
        QuotaSubject::value(value.to_vec()).unwrap(),
        None,
    )
    .unwrap()
}

fn token_policy(capacity: u64) -> RatePolicy {
    token_policy_with_interval(capacity, 10)
}

fn token_policy_with_interval(capacity: u64, interval_nanos: u64) -> RatePolicy {
    TokenBucketPolicy::new(capacity, 1, Duration::from_nanos(interval_nanos))
        .unwrap()
        .into()
}

fn decide(
    limiter: &impl RateLimiter,
    key: &QuotaKey,
    cost: u64,
    policy: RatePolicy,
) -> RateLimitDecision {
    poll(limiter.check_and_consume(RateLimitRequest::new(
        key,
        OperationCost::new(cost).unwrap(),
        policy,
    )))
    .unwrap()
}

fn config(max_keys: usize, idle_ttl: Duration) -> InMemoryRateLimitConfig {
    InMemoryRateLimitConfig::new(max_keys, idle_ttl).unwrap()
}

fn poll(
    future: RateLimitFuture<'_>,
) -> Result<RateLimitDecision, chigiri_rate::RateLimitProviderError> {
    let waker = std::task::Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = future;
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

struct ThreadWake(std::thread::Thread);

impl std::task::Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn assert_allowed(decision: RateLimitDecision) {
    assert!(decision.is_allowed());
}

fn assert_rejected(decision: RateLimitDecision) {
    assert!(!decision.is_allowed());
}

fn snapshot(decision: RateLimitDecision) -> RateLimitSnapshot {
    decision.snapshot()
}

fn retry(decision: RateLimitDecision) -> u64 {
    assert_rejected(decision);
    decision.snapshot().retry_after().unwrap().as_nanos() as u64
}

fn reject_reason(decision: RateLimitDecision) -> RateLimitRejectReason {
    assert_rejected(decision);
    decision.snapshot().reject_reason().unwrap()
}
