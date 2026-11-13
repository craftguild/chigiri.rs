//! Tests equivalence between the convenience API and direct memory-provider injection.

use std::time::Duration;

use super::rate_support::{OperationKey, code};
use super::support::{execute, resolvers};

#[test]
fn rmem_13_convenience_and_explicit_memory_injection_produce_the_same_stage() {
    let sugar_stage = chigiri::rate::operation(OperationKey)
        .fixed_window()
        .limit(1)
        .unwrap()
        .window(Duration::from_secs(60))
        .unwrap()
        .build()
        .unwrap();
    let sugar = chigiri::Endpoint::builder(resolvers())
        .rate_limit(sugar_stage)
        .without_context()
        .build()
        .unwrap();

    let policy: chigiri::rate::RatePolicy =
        chigiri::rate::FixedWindowPolicy::new(1, Duration::from_secs(60))
            .unwrap()
            .into();
    let deep_stage = chigiri::rate::OperationRateLimit::new(
        OperationKey,
        chigiri::rate::UnitFieldCost,
        chigiri::rate::memory::InMemoryFixedWindow::new(),
        policy,
        chigiri::rate::ProviderFailurePolicy::FailClosed,
        chigiri::rate::OperationCost::one(),
    )
    .unwrap()
    .with_provider_timeout(Duration::from_millis(500))
    .unwrap();
    let injected = chigiri::Endpoint::builder(resolvers())
        .inject(|components| {
            components.rate_limit().stage_b_provider(deep_stage);
        })
        .without_context()
        .build()
        .unwrap();

    assert!(sugar.configuration().stage_b_rate_limit().is_some());
    assert!(injected.configuration().stage_b_rate_limit().is_none());
    assert!(execute(&sugar, "{ greeting }").errors.is_empty());
    assert!(execute(&injected, "{ greeting }").errors.is_empty());
    assert_eq!(code(&execute(&sugar, "{ greeting }")), Some("RATE_LIMITED"));
    assert_eq!(
        code(&execute(&injected, "{ greeting }")),
        Some("RATE_LIMITED")
    );
}
