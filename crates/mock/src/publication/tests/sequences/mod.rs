//! Prove direct timed delivery shares queues and admission without joining pub/sub.

mod preflight;
mod support;

use std::task::Poll;
use std::time::{Duration, Instant};

use chigiri::SubscriptionOverflow;

use crate::publication::{PublicationError, PublicationPump};
use support::{Fixture, event, poll};

#[test]
fn finite_source_drains_accepted_values_without_cancelling_its_executing_input() {
    let fixture = Fixture::new(4, 1);
    let definition = fixture.definition(false);
    let origin = Instant::now();
    let (mut source, counter) =
        fixture.register(&definition, SubscriptionOverflow::Backpressure, origin);
    let mut pump = PublicationPump::new();
    fixture.drive(&mut pump, origin + Duration::from_millis(9));
    assert!(poll(&mut source).is_pending());
    fixture.drive(&mut pump, origin + Duration::from_millis(10));
    let first = event(&mut source);
    assert_eq!(fixture.value(&first), 1);
    fixture.drive(&mut pump, origin + Duration::from_secs(10));
    let last = event(&mut source);
    assert_eq!(fixture.value(&last), 3);
    assert!(!first.cancellation().unwrap().cancelled());
    assert!(!last.cancellation().unwrap().cancelled());
    assert_eq!(counter.snapshot(), Some(0));
    assert!(!counter.is_stopped());
    assert!(matches!(poll(&mut source), Poll::Ready(None)));
    let foreign = Fixture::new(4, 1);
    assert!(fixture.world.lock().unwrap().owns_event(&first));
    assert!(!foreign.world.lock().unwrap().owns_event(&first));
    let (_, retention) = fixture.world.lock().unwrap().source_policy();
    drop((source, definition, counter));
    fixture.drive(&mut pump, origin + Duration::from_secs(10));
    assert!(retention.used() > 0);
    drop((first, last));
    assert_eq!(retention.used(), 0);
}

#[test]
fn origins_are_independent_and_sequence_values_never_enter_the_topic_channel() {
    let fixture = Fixture::new(4, 4);
    let definition = fixture.definition(true);
    let origin = Instant::now();
    let (mut first, _) = fixture.register(&definition, SubscriptionOverflow::Backpressure, origin);
    let (mut second, _) = fixture.register(
        &definition,
        SubscriptionOverflow::Backpressure,
        origin + Duration::from_millis(15),
    );
    let mut topic = fixture.pubsub(false);
    let mut pump = PublicationPump::new();
    fixture.drive(&mut pump, origin + Duration::from_millis(20));
    assert_eq!(fixture.value(&event(&mut first)), 2);
    assert!(poll(&mut second).is_pending());
    assert!(poll(&mut topic).is_pending());
    fixture.drive(&mut pump, origin + Duration::from_millis(25));
    assert!(poll(&mut first).is_pending());
    assert_eq!(fixture.value(&event(&mut second)), 1);
    fixture.publish(99);
    fixture.drive(&mut pump, origin + Duration::from_millis(25));
    assert_eq!(fixture.value(&event(&mut topic)), 99);
    assert!(poll(&mut first).is_pending());
    assert!(poll(&mut second).is_pending());
    assert_eq!(fixture.world.lock().unwrap().topics.counts(), (1, 3));
    drop(topic);
    fixture.drive(&mut pump, origin + Duration::from_millis(25));
    assert_eq!(fixture.world.lock().unwrap().topics.counts(), (0, 2));
    let mut all = fixture.pubsub(true);
    drop(first);
    fixture.drive(&mut pump, origin + Duration::from_millis(25));
    assert_eq!(fixture.world.lock().unwrap().topics.counts(), (1, 2));
    fixture.publish(100);
    fixture.drive(&mut pump, origin + Duration::from_millis(25));
    assert_eq!(fixture.value(&event(&mut all)), 100);
}

#[test]
fn non_demanded_sequences_share_overflow_policy_but_skipped_ticks_are_not_drops() {
    let fixture = Fixture::new(2, 2);
    let definition = fixture.definition(true);
    let origin = Instant::now();
    let (mut stopped, stopped_count) =
        fixture.register(&definition, SubscriptionOverflow::Backpressure, origin);
    let (mut dropping, dropping_count) =
        fixture.register(&definition, SubscriptionOverflow::DropOldest, origin);
    let mut pump = PublicationPump::new();
    for milliseconds in [10, 20, 30, 100] {
        fixture.drive(&mut pump, origin + Duration::from_millis(milliseconds));
    }
    assert!(stopped_count.is_stopped());
    assert_eq!(stopped_count.snapshot(), Some(0));
    assert!(matches!(poll(&mut stopped), Poll::Ready(Some(Err(_)))));
    assert_eq!(dropping_count.snapshot(), Some(2));
    let first = event(&mut dropping);
    let second = event(&mut dropping);
    assert_eq!((fixture.value(&first), fixture.value(&second)), (3, 1));
    assert_eq!(
        (first.dropped_total(), second.dropped_total()),
        (Some(2), Some(2))
    );
    assert!(!dropping_count.is_stopped());
}

#[test]
fn sequence_and_pubsub_share_physical_admission_until_the_last_executing_input_leaves() {
    let fixture = Fixture::new(4, 1);
    let definition = fixture.definition(false);
    let origin = Instant::now();
    let (mut source, _) = fixture.register(&definition, SubscriptionOverflow::Backpressure, origin);
    let mut pump = PublicationPump::new();
    fixture.drive(&mut pump, origin + Duration::from_millis(30));
    let held = event(&mut source);
    drop(source);
    fixture.drive(&mut pump, origin + Duration::from_millis(30));
    assert_eq!(fixture.world.lock().unwrap().topics.counts(), (0, 0));
    let owner = fixture.world.lock().unwrap().registration_owner();
    let prepared = owner.prepare(&fixture.ir, fixture.field, "events").unwrap();
    let failed = fixture
        .world
        .lock()
        .unwrap()
        .register_source(prepared, SubscriptionOverflow::Backpressure)
        .err()
        .unwrap();
    assert_eq!(failed.finish(), PublicationError::BindingCapacity);
    drop(held);
    drop(fixture.pubsub(false));
    fixture.drive(&mut pump, origin + Duration::from_millis(30));
    assert_eq!(fixture.world.lock().unwrap().topics.counts(), (0, 0));
}
