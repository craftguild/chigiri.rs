//! Typestate builders that keep algorithm-specific rate-limit values separate.

use std::marker::PhantomData;
use std::time::Duration;

use chigiri_policy::{OperationRateLimit, ProviderFailurePolicy, StageARateLimit, UnitFieldCost};
use chigiri_rate::{
    FixedWindowPolicy, LeakyBucketPolicy, OperationCost, RateLimitPolicyError, RateLimiter,
    RatePolicy, SlidingWindowPolicy, TokenBucketPolicy,
};
use chigiri_rate_memory::{
    InMemoryFixedWindow, InMemoryLeakyBucket, InMemorySlidingWindowCounter, InMemoryTokenBucket,
    SystemClock,
};

use super::{OperationRateStage, RateStageBuildError, TransportRateStage};

const DEFAULT_PROVIDER_TIMEOUT: Duration = Duration::from_millis(500);

#[doc(hidden)]
pub struct Transport;
#[doc(hidden)]
pub struct Operation;

/// Algorithm selection builder for transport Stage A.
pub struct TransportBuilder<E> {
    extractor: E,
}

/// Algorithm-selection builder for operation Stage B.
pub struct OperationBuilder<E> {
    extractor: E,
}

/// Starts selection of a transport Stage A algorithm.
pub const fn transport<E>(extractor: E) -> TransportBuilder<E> {
    TransportBuilder { extractor }
}

/// Starts selection of an operation Stage B algorithm.
pub const fn operation<E>(extractor: E) -> OperationBuilder<E> {
    OperationBuilder { extractor }
}

macro_rules! choices {
    ($builder:ident, $kind:ident) => {
        impl<E> $builder<E> {
            /// Selects a burst-capable token bucket.
            pub fn token_bucket(self) -> AlgorithmBuilder<$kind, E, TokenBucket> {
                AlgorithmBuilder::new(self.extractor, TokenBucket::default())
            }
            /// Select a fixed window to update the quota for each boundary.
            pub fn fixed_window(self) -> AlgorithmBuilder<$kind, E, FixedWindow> {
                AlgorithmBuilder::new(self.extractor, FixedWindow::default())
            }
            /// Select the sliding-window counter to load the front window.
            pub fn sliding_window_counter(self) -> AlgorithmBuilder<$kind, E, SlidingWindow> {
                AlgorithmBuilder::new(self.extractor, SlidingWindow::default())
            }
            /// Selects a leaky-bucket meter without a waiting queue.
            pub fn leaky_bucket(self) -> AlgorithmBuilder<$kind, E, LeakyBucket> {
                AlgorithmBuilder::new(self.extractor, LeakyBucket::default())
            }
        }
    };
}

choices!(TransportBuilder, Transport);
choices!(OperationBuilder, Operation);

/// Typestate indicating that an algorithm has been selected.
pub struct AlgorithmBuilder<K, E, A> {
    extractor: E,
    algorithm: A,
    failure_policy: ProviderFailurePolicy,
    provider_timeout: Duration,
    marker: PhantomData<fn() -> K>,
}

impl<K, E, A> AlgorithmBuilder<K, E, A> {
    fn new(extractor: E, algorithm: A) -> Self {
        Self {
            extractor,
            algorithm,
            failure_policy: ProviderFailurePolicy::FailClosed,
            provider_timeout: DEFAULT_PROVIDER_TIMEOUT,
            marker: PhantomData,
        }
    }

    /// Replaces the default fail-closed provider policy.
    pub const fn failure_policy(mut self, policy: ProviderFailurePolicy) -> Self {
        self.failure_policy = policy;
        self
    }

    /// Replaces the default 500 ms provider deadline.
    pub fn provider_timeout(mut self, timeout: Duration) -> Result<Self, RateStageBuildError> {
        check_provider_timeout(timeout)?;
        self.provider_timeout = timeout;
        Ok(self)
    }
}

#[derive(Default)]
/// The marker that keeps the unfinished settings of the token bucket.
pub struct TokenBucket {
    capacity: Option<u64>,
    refill: Option<(u64, Duration)>,
}

impl<K, E> AlgorithmBuilder<K, E, TokenBucket> {
    /// Sets the bucket's maximum token capacity.
    pub fn capacity(mut self, capacity: u64) -> Result<Self, RateStageBuildError> {
        check_nonzero(capacity)?;
        self.algorithm.capacity = Some(capacity);
        Ok(self)
    }
    /// Sets tokens added per refill interval.
    pub fn refill(mut self, units: u64, interval: Duration) -> Result<Self, RateStageBuildError> {
        check_nonzero(units)?;
        check_duration(interval)?;
        self.algorithm.refill = Some((units, interval));
        Ok(self)
    }
}

#[derive(Default)]
/// Marker that keeps the unfinished settings of fixed window.
pub struct FixedWindow {
    limit: Option<u64>,
    window: Option<Duration>,
}

#[derive(Default)]
/// The marker that keeps the unfinished settings of the sliding-window counter.
pub struct SlidingWindow {
    limit: Option<u64>,
    window: Option<Duration>,
}

macro_rules! limit_window {
    ($algorithm:ident) => {
        impl<K, E> AlgorithmBuilder<K, E, $algorithm> {
            /// Set the maximum cost allowed in one window.
            pub fn limit(mut self, limit: u64) -> Result<Self, RateStageBuildError> {
                check_nonzero(limit)?;
                self.algorithm.limit = Some(limit);
                Ok(self)
            }
            /// Sets the fixed-window width used to replenish quota.
            pub fn window(mut self, window: Duration) -> Result<Self, RateStageBuildError> {
                check_duration(window)?;
                self.algorithm.window = Some(window);
                Ok(self)
            }
        }
    };
}

limit_window!(FixedWindow);
limit_window!(SlidingWindow);

#[derive(Default)]
/// The markers that keep the leaky bucket unfinished settings.
pub struct LeakyBucket {
    capacity: Option<u64>,
    drain: Option<(u64, Duration)>,
}

impl<K, E> AlgorithmBuilder<K, E, LeakyBucket> {
    /// Sets the bucket's maximum accumulated cost.
    pub fn capacity(mut self, capacity: u64) -> Result<Self, RateStageBuildError> {
        check_nonzero(capacity)?;
        self.algorithm.capacity = Some(capacity);
        Ok(self)
    }
    /// Sets the cost drained per interval.
    pub fn drain(mut self, units: u64, interval: Duration) -> Result<Self, RateStageBuildError> {
        check_nonzero(units)?;
        check_duration(interval)?;
        self.algorithm.drain = Some((units, interval));
        Ok(self)
    }
}

#[doc(hidden)]
pub trait AlgorithmSpec {
    type Provider: RateLimiter;
    fn build_provider(self) -> Result<(Self::Provider, RatePolicy), RateStageBuildError>;
}

impl<E, A> AlgorithmBuilder<Transport, E, A>
where
    A: AlgorithmSpec,
{
    /// Build a Stage A component that owns its process-local provider.
    pub fn build(self) -> Result<TransportRateStage<StageARateLimit<E>>, RateStageBuildError> {
        let (provider, policy) = self.algorithm.build_provider()?;
        let admission = StageARateLimit::new(self.extractor, provider, policy, self.failure_policy)
            .with_provider_timeout(self.provider_timeout)?;
        Ok(TransportRateStage::new(
            admission,
            self.provider_timeout,
            self.failure_policy,
            policy,
        ))
    }
}

impl<E, A> AlgorithmBuilder<Operation, E, A>
where
    A: AlgorithmSpec,
{
    /// Build a Stage B component that owns its process-local provider.
    pub fn build(self) -> Result<OperationRateStage<OperationRateLimit<E>>, RateStageBuildError> {
        let (provider, policy) = self.algorithm.build_provider()?;
        let maximum = OperationCost::new(policy.limit())
            .map_err(|_| RateStageBuildError::MissingSetting("positive operation maximum"))?;
        let plugin = OperationRateLimit::new(
            self.extractor,
            UnitFieldCost,
            provider,
            policy,
            self.failure_policy,
            maximum,
        )?
        .with_provider_timeout(self.provider_timeout)?;
        Ok(OperationRateStage::new(
            plugin,
            self.provider_timeout,
            self.failure_policy,
            policy,
        ))
    }
}

impl AlgorithmSpec for TokenBucket {
    type Provider = InMemoryTokenBucket<SystemClock>;
    fn build_provider(self) -> Result<(Self::Provider, RatePolicy), RateStageBuildError> {
        let capacity = self
            .capacity
            .ok_or(RateStageBuildError::MissingSetting("capacity"))?;
        let (units, interval) = self
            .refill
            .ok_or(RateStageBuildError::MissingSetting("refill"))?;
        let policy = TokenBucketPolicy::new(capacity, units, interval)?;
        Ok((InMemoryTokenBucket::new(), policy.into()))
    }
}

impl AlgorithmSpec for FixedWindow {
    type Provider = InMemoryFixedWindow<SystemClock>;
    fn build_provider(self) -> Result<(Self::Provider, RatePolicy), RateStageBuildError> {
        let limit = self
            .limit
            .ok_or(RateStageBuildError::MissingSetting("limit"))?;
        let window = self
            .window
            .ok_or(RateStageBuildError::MissingSetting("window"))?;
        let policy = FixedWindowPolicy::new(limit, window)?;
        Ok((InMemoryFixedWindow::new(), policy.into()))
    }
}

impl AlgorithmSpec for SlidingWindow {
    type Provider = InMemorySlidingWindowCounter<SystemClock>;
    fn build_provider(self) -> Result<(Self::Provider, RatePolicy), RateStageBuildError> {
        let limit = self
            .limit
            .ok_or(RateStageBuildError::MissingSetting("limit"))?;
        let window = self
            .window
            .ok_or(RateStageBuildError::MissingSetting("window"))?;
        let policy = SlidingWindowPolicy::new(limit, window)?;
        Ok((InMemorySlidingWindowCounter::new(), policy.into()))
    }
}

impl AlgorithmSpec for LeakyBucket {
    type Provider = InMemoryLeakyBucket<SystemClock>;
    fn build_provider(self) -> Result<(Self::Provider, RatePolicy), RateStageBuildError> {
        let capacity = self
            .capacity
            .ok_or(RateStageBuildError::MissingSetting("capacity"))?;
        let (units, interval) = self
            .drain
            .ok_or(RateStageBuildError::MissingSetting("drain"))?;
        let policy = LeakyBucketPolicy::new(capacity, units, interval)?;
        Ok((InMemoryLeakyBucket::new(), policy.into()))
    }
}

fn check_nonzero(value: u64) -> Result<(), RateStageBuildError> {
    if value == 0 {
        Err(RateLimitPolicyError.into())
    } else {
        Ok(())
    }
}

fn check_duration(value: Duration) -> Result<(), RateStageBuildError> {
    if value.is_zero() || value.as_nanos() > u128::from(u64::MAX) {
        Err(RateLimitPolicyError.into())
    } else {
        Ok(())
    }
}

fn check_provider_timeout(value: Duration) -> Result<(), RateStageBuildError> {
    if value.is_zero() || value.as_nanos() > u128::from(u64::MAX) {
        Err(chigiri_policy::ProviderTimeoutConfigurationError.into())
    } else {
        Ok(())
    }
}
