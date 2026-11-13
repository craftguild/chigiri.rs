//! Activation must fit source metadata, timer ownership and one delivery envelope together.

use std::time::{Duration, Instant};

use chigiri::SubscriptionOverflow;

use super::support::{Fixture, event};
use crate::publication::{CapturedEvent, PublicationError, PublicationPump};

#[test]
fn sequence_activation_preflight_accepts_exact_capacity_after_timer_allocation() {
    let fixture = Fixture::new(4, 1);
    let definition = fixture.definition(false);
    let (limits, retention) = fixture.world.lock().unwrap().source_policy();
    let owner = fixture.world.lock().unwrap().registration_owner();
    let baseline = retention.used();
    let prepared = owner
        .prepare_sequence(&fixture.ir, fixture.field, &definition)
        .unwrap();
    let metadata = retention.used() - baseline;
    drop(prepared);
    assert_eq!(retention.used(), baseline);
    let envelope = CapturedEvent::envelope_bytes();
    let pin_bytes = limits.max_retained_bytes() - baseline - metadata - envelope;
    let too_full = retention.reserve(pin_bytes + 1, usize::MAX).unwrap();
    assert_eq!(
        owner
            .prepare_sequence(&fixture.ir, fixture.field, &definition)
            .err(),
        Some(PublicationError::CaptureLimit)
    );
    assert_eq!(retention.used(), baseline + pin_bytes + 1);
    drop(too_full);
    let exact = retention.reserve(pin_bytes, usize::MAX).unwrap();
    let prepared = owner
        .prepare_sequence(&fixture.ir, fixture.field, &definition)
        .unwrap();
    let origin = Instant::now();
    let registered = fixture
        .world
        .lock()
        .unwrap()
        .register_source(prepared, SubscriptionOverflow::Backpressure)
        .ok()
        .unwrap()
        .finish_at(origin);
    let (mut source, _) = registered.into_parts();
    let mut pump = PublicationPump::new();
    fixture.drive(&mut pump, origin + Duration::from_millis(10));
    let held = event(&mut source);
    assert_eq!(fixture.value(&held), 1);
    drop((source, definition));
    fixture.drive(&mut pump, origin + Duration::from_millis(10));
    drop(held);
    assert_eq!(retention.used(), pin_bytes);
    drop(exact);
    assert_eq!(retention.used(), 0);
}
