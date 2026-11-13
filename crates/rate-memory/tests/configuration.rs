use std::sync::Arc;
use std::time::Duration;

use chigiri_rate::MonotonicTick;
use chigiri_rate_memory::{
    InMemoryFixedWindow, InMemoryLeakyBucket, InMemoryRateLimitConfig,
    InMemorySlidingWindowCounter, InMemoryTokenBucket, ManualClock,
};

#[test]
fn rmem_02_safe_defaults_work_with_every_constructor() {
    let config = InMemoryRateLimitConfig::default();
    assert_eq!(config.max_keys(), 10_000);
    assert_eq!(config.idle_ttl(), Duration::from_secs(300));

    let _: InMemoryTokenBucket = InMemoryTokenBucket::new();
    let _: InMemoryFixedWindow = InMemoryFixedWindow::default();
    let _: InMemorySlidingWindowCounter = InMemorySlidingWindowCounter::with_config(config);
    let _: InMemoryLeakyBucket<_> = InMemoryLeakyBucket::with_clock(
        config,
        Arc::new(ManualClock::new(MonotonicTick::from_nanos(1))),
    );
}

#[test]
fn rmem_03_config_debug_redacts_values_and_rejects_zero_or_unbounded_durations() {
    assert!(InMemoryRateLimitConfig::new(0, Duration::from_secs(1)).is_err());
    assert!(InMemoryRateLimitConfig::new(1, Duration::ZERO).is_err());
    assert!(InMemoryRateLimitConfig::new(1, Duration::MAX).is_err());

    let config = InMemoryRateLimitConfig::new(9_876_543, Duration::from_nanos(7_654_321)).unwrap();
    let debug = format!("{config:?}");
    assert!(!debug.contains("9876543"));
    assert!(!debug.contains("7654321"));
    let clock = ManualClock::new(MonotonicTick::from_nanos(6_543_210));
    assert!(!format!("{clock:?}").contains("6543210"));
}
