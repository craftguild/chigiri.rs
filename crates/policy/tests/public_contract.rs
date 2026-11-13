use chigiri_policy::{OperationCostPolicy, UnitFieldCost};
use chigiri_rate::{OperationCost, QuotaKey, QuotaSubject, RateLimiter};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn cost_key_limiter_does_not_require_a_core_provider_type() {
    let policy: &dyn OperationCostPolicy = &UnitFieldCost;
    let limiter: Option<&dyn RateLimiter> = None;
    let key = QuotaKey::new(
        b"tenant".to_vec(),
        QuotaSubject::value(b"hashed-subject".to_vec()).unwrap(),
        Some(b"query".to_vec()),
    )
    .unwrap();
    assert_eq!(OperationCost::one().get(), 1);
    assert!(limiter.is_none());
    assert_send_sync::<QuotaKey>();
    let _ = policy;
    let _ = key;
}
