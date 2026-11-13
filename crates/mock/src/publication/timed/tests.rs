//! Check cadence arithmetic without depending on platform-sized Instant ranges.

use super::*;
use crate::events::{SourceLimits, retention::Retention};
use crate::output::OutputValue;
use crate::publication::DefinitionBuilder;
use crate::store::Store;
use crate::value::ValueBudget;

fn sequence(looping: bool) -> (TimedSequence, Store, crate::ir::FieldId) {
    let ir = Arc::new(
        crate::ir::MockIr::parse(vec![chigiri::core::Source::new(
            "timed.graphql",
            "type Query { ping: Int } type Subscription { tick: Int! }",
        )])
        .unwrap(),
    );
    let field = ir.graph().types()[ir.graph().subscription().unwrap().0].fields["tick"];
    let limits = SourceLimits::default();
    let retention = Retention::new(limits.max_retained_bytes());
    let mut definition =
        DefinitionBuilder::new(&ir, field, 3, 10, looping, limits, &retention).unwrap();
    for value in 1..=3 {
        definition
            .push(&ir, &OutputValue::Scalar(value.into()))
            .unwrap();
    }
    (
        TimedSequence::new(definition.seal().unwrap()),
        Store::empty(ir),
        field,
    )
}

fn value(tick: Tick, store: &Store, field: crate::ir::FieldId) -> i128 {
    let OutputValue::Scalar(chigiri::Value::Int(value)) = tick
        .event
        .project(store.snapshot(), field, &mut ValueBudget::default())
        .unwrap()
    else {
        panic!("integer tick required")
    };
    value
}

#[test]
fn cadence_starts_after_one_interval_skips_late_positions_and_never_repeats_a_tick() {
    let (mut sequence, store, field) = sequence(true);
    let origin = Instant::now();
    assert!(
        sequence
            .tick(None, origin + Duration::from_secs(1))
            .unwrap()
            .is_none()
    );
    assert!(
        sequence
            .tick(Some(origin), origin + Duration::from_micros(9_999))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        value(
            sequence
                .tick(Some(origin), origin + Duration::from_millis(10))
                .unwrap()
                .unwrap(),
            &store,
            field
        ),
        1
    );
    assert!(
        sequence
            .tick(Some(origin), origin + Duration::from_millis(10))
            .unwrap()
            .is_none()
    );
    let late = sequence
        .tick(Some(origin), origin + Duration::from_millis(89))
        .unwrap()
        .unwrap();
    assert!(!late.complete);
    assert_eq!(late.event.dropped_total(), None);
    assert_eq!(value(late, &store, field), 2);
    assert_eq!(
        sequence.deadline(),
        Some(origin + Duration::from_millis(90))
    );
    assert!(
        sequence
            .tick(Some(origin), origin + Duration::from_millis(89))
            .unwrap()
            .is_none()
    );
}

#[test]
fn finite_late_wakeup_emits_only_the_final_value_once() {
    let (mut sequence, store, field) = sequence(false);
    let tick = sequence.select(u128::MAX).unwrap().unwrap();
    assert!(tick.complete);
    assert_eq!(value(tick, &store, field), 3);
    assert!(sequence.select(u128::MAX).unwrap().is_none());
    assert_eq!(sequence.deadline(), None);
}

#[test]
fn looping_tick_ceiling_accepts_equality_and_stops_before_precision_is_lost() {
    let (mut sequence, store, field) = sequence(true);
    let tick = sequence
        .select(u128::from(MAX_SAFE_SEQUENCE))
        .unwrap()
        .unwrap();
    assert_eq!(
        value(tick, &store, field),
        i128::from((MAX_SAFE_SEQUENCE - 1) % 3 + 1)
    );
    assert_eq!(
        sequence.select(u128::from(MAX_SAFE_SEQUENCE) + 1).err(),
        Some(PublicationError::SequenceExhausted)
    );
    assert_eq!(
        sequence.select(u128::MAX).err(),
        Some(PublicationError::SequenceExhausted)
    );
}
