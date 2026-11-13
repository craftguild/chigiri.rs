use super::*;

mod store {
    use super::*;

    #[test]
    fn limit_05_bounds_keys_and_expiry_for_all_four_algorithms() {
        let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(100)));
        let token = InMemoryTokenBucket::with_clock(
            config(1, Duration::from_nanos(10)),
            Arc::clone(&clock),
        );
        bounded_store(&token, &clock, token_policy(2), b"token");
        assert_eq!(token.entry_count().unwrap(), 1);

        let fixed = InMemoryFixedWindow::with_clock(
            config(1, Duration::from_nanos(10)),
            Arc::clone(&clock),
        );
        let fixed_policy = FixedWindowPolicy::new(2, Duration::from_nanos(100))
            .unwrap()
            .into();
        bounded_store(&fixed, &clock, fixed_policy, b"fixed");
        assert_eq!(fixed.entry_count().unwrap(), 1);

        let sliding = InMemorySlidingWindowCounter::with_clock(
            config(1, Duration::from_nanos(10)),
            Arc::clone(&clock),
        );
        let sliding_policy = SlidingWindowPolicy::new(2, Duration::from_nanos(100))
            .unwrap()
            .into();
        bounded_store(&sliding, &clock, sliding_policy, b"sliding");
        assert_eq!(sliding.entry_count().unwrap(), 1);

        let leaky = InMemoryLeakyBucket::with_clock(
            config(1, Duration::from_nanos(10)),
            Arc::clone(&clock),
        );
        let leaky_policy = LeakyBucketPolicy::new(2, 1, Duration::from_nanos(10))
            .unwrap()
            .into();
        bounded_store(&leaky, &clock, leaky_policy, b"leaky");
        assert_eq!(leaky.entry_count().unwrap(), 1);
    }

    #[test]
    fn quota_rejection_keeps_active_keys_from_idle_expiry_for_all_four_algorithms() {
        let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(0)));
        let ttl = Duration::from_nanos(10);
        let token = InMemoryTokenBucket::with_clock(config(1, ttl), Arc::clone(&clock));
        active_reject(&token, &clock, token_policy_with_interval(1, 100), b"token");

        clock.set(MonotonicTick::from_nanos(0));
        let fixed = InMemoryFixedWindow::with_clock(config(1, ttl), Arc::clone(&clock));
        let policy = FixedWindowPolicy::new(1, Duration::from_nanos(100))
            .unwrap()
            .into();
        active_reject(&fixed, &clock, policy, b"fixed");

        clock.set(MonotonicTick::from_nanos(0));
        let sliding = InMemorySlidingWindowCounter::with_clock(config(1, ttl), Arc::clone(&clock));
        let policy = SlidingWindowPolicy::new(1, Duration::from_nanos(100))
            .unwrap()
            .into();
        active_reject(&sliding, &clock, policy, b"sliding");

        clock.set(MonotonicTick::from_nanos(0));
        let leaky = InMemoryLeakyBucket::with_clock(config(1, ttl), Arc::clone(&clock));
        let policy = LeakyBucketPolicy::new(1, 1, Duration::from_nanos(100))
            .unwrap()
            .into();
        active_reject(&leaky, &clock, policy, b"leaky");
    }

    #[test]
    fn limit_06_capacity_and_policy_mismatches_do_not_change_state() {
        let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(100)));
        let limiter = InMemoryTokenBucket::with_clock(
            config(1, Duration::from_nanos(10)),
            Arc::clone(&clock),
        );
        let original = token_policy(2);
        let changed: RatePolicy = TokenBucketPolicy::new(3, 1, Duration::from_nanos(10))
            .unwrap()
            .into();
        let first = key(b"first");
        let second = key(b"second");
        assert_allowed(decide(&limiter, &first, 1, original));
        assert_eq!(
            reject_reason(decide(&limiter, &first, 1, changed)),
            RateLimitRejectReason::PolicyMismatch
        );
        assert_eq!(
            reject_reason(decide(&limiter, &second, 1, original)),
            RateLimitRejectReason::KeyCapacityExhausted
        );
        assert_allowed(decide(&limiter, &first, 1, original));
        assert_eq!(limiter.entry_count().unwrap(), 1);
    }
}

mod clock {
    use super::*;

    #[test]
    fn limit_13_handles_clock_rollback_and_large_forward_jumps() {
        let clock = Arc::new(ManualClock::new(MonotonicTick::from_nanos(10)));
        let limiter = InMemoryTokenBucket::with_clock(
            config(1, Duration::from_nanos(100)),
            Arc::clone(&clock),
        );
        let policy = token_policy(2);
        let subject = key(b"clock");
        assert_allowed(decide(&limiter, &subject, 2, policy));
        assert_rejected(decide(&limiter, &subject, 1, policy));
        clock.set(MonotonicTick::from_nanos(1));
        assert_rejected(decide(&limiter, &subject, 1, policy));
        clock.set(MonotonicTick::from_nanos(u64::MAX));
        assert_allowed(decide(&limiter, &subject, 1, policy));
        assert_eq!(limiter.entry_count().unwrap(), 1);
    }
}
