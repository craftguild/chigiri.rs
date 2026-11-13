//! Deterministic typed queue boundaries include executing physical ownership.

use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use chigiri::SubscriptionOverflow;

use super::super::retained::RetainedBatch;
use super::super::source::SourcePort;
use super::super::source::admission::Admission;
use super::super::{CapturedEvent, PublicationLimits, PublicationStore};
use super::support::{Fixture, fixture, scalar};
use crate::events::{SourceLimits, retention::Retention};
use crate::store::Store;
use crate::value::ValueBudget;

struct Noop;
impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn poll(
    port: &SourcePort,
    now: Instant,
) -> Poll<Option<Result<CapturedEvent, crate::luau::ScriptError>>> {
    port.poll_at(&mut Context::from_waker(&Waker::from(Arc::new(Noop))), now)
}

fn event(port: &SourcePort, now: Instant) -> CapturedEvent {
    let Poll::Ready(Some(Ok(event))) = poll(port, now) else {
        panic!("queued event required")
    };
    event
}

fn port(limits: SourceLimits, budget: &Retention, dropping: bool) -> Arc<SourcePort> {
    let port = SourcePort::new(limits, budget.clone()).unwrap();
    port.configure(if dropping {
        SubscriptionOverflow::DropOldest
    } else {
        SubscriptionOverflow::Backpressure
    })
    .unwrap();
    port
}

fn limits(count: usize, bytes: usize, age: u64) -> SourceLimits {
    SourceLimits::new(count, bytes, age, 16 * 1024 * 1024, 1024 * 1024).unwrap()
}

fn capture(store: &PublicationStore, fixture: &Fixture, value: &str) -> Arc<RetainedBatch> {
    let buffer = store
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap();
    buffer
        .push(&fixture.ir, fixture.event, "events", &scalar(value))
        .unwrap();
    buffer.prepare(&store.staging()).unwrap().unwrap().retained
}

fn world(fixture: &Fixture) -> PublicationStore {
    PublicationStore::new(Store::empty(fixture.ir.clone()), 8).unwrap()
}

#[test]
fn default_overflow_cancels_only_the_full_source_and_keeps_executing_bytes() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let slow = port(limits(1, 4096, 1000), &budget, false);
    let healthy = port(limits(1, 4096, 1000), &budget, false);
    let now = Instant::now();
    let batch = capture(&store, &fixture, "one");
    slow.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    healthy.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    let running = event(&slow, now);
    let peer = event(&healthy, now);
    assert!(!running.cancellation().unwrap().cancelled());
    slow.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    // The executing input does not consume the single waiting-event slot.
    assert!(!slow.is_closed());
    slow.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    assert!(slow.is_closed());
    assert!(running.cancellation().unwrap().cancelled());
    assert!(!peer.cancellation().unwrap().cancelled());
    assert!(matches!(poll(&slow, now), Poll::Ready(Some(Err(_)))));
    assert!(matches!(poll(&slow, now), Poll::Ready(None)));
    assert!(poll(&healthy, now).is_pending());
    drop(batch);
    drop(peer);
    drop(slow);
    drop(healthy);
    assert!(budget.used() > 0);
    drop(running);
    assert_eq!(budget.used(), 0);
}

#[test]
fn drop_oldest_has_independent_fifo_and_counts_only_wholly_waiting_evictions() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let a = port(limits(1, 4096, 1000), &budget, true);
    let b = port(limits(2, 4096, 1000), &budget, true);
    let now = Instant::now();
    for value in ["one", "two"] {
        let batch = capture(&store, &fixture, value);
        a.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
        b.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    }
    let received = [event(&a, now), event(&b, now), event(&b, now)];
    for (event, (value, drops)) in received.iter().zip([("two", 1), ("one", 0), ("two", 0)]) {
        let output = event
            .project(store.snapshot(), fixture.event, &mut ValueBudget::default())
            .unwrap();
        assert!(
            matches!(output,crate::output::OutputValue::Scalar(chigiri::Value::String(ref text)) if text==value)
        );
        assert_eq!(event.dropped_total(), Some(drops));
    }
    assert!(poll(&a, now).is_pending());
    assert!(poll(&b, now).is_pending());
    drop(a);
    drop(b);
    assert!(budget.used() > 0);
    drop(received);
    assert_eq!(budget.used(), 0);
}

#[test]
fn retained_bytes_include_the_executing_input_and_cannot_evict_it() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let batch = capture(&store, &fixture, "one");
    let size = RetainedBatch::event(&batch, 0).unwrap().source_bytes();
    let port = port(limits(10, size, 1000), &budget, true);
    let now = Instant::now();
    port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    let running = event(&port, now);
    assert_eq!(running.dropped_total(), Some(0));
    port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    assert!(port.is_closed());
    assert!(running.cancellation().unwrap().cancelled());
    assert_eq!(running.dropped_total(), Some(0));
    assert!(matches!(poll(&port, now), Poll::Ready(Some(Err(_)))));
}

#[test]
fn age_equality_is_valid_and_idle_expiry_drops_or_stops_without_demand() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let start = Instant::now();
    let boundary = start + Duration::from_millis(5);
    for dropping in [false, true] {
        let port = port(limits(2, 4096, 5), &budget, dropping);
        let batch = capture(&store, &fixture, "one");
        port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), start);
        port.expire(boundary).unwrap();
        assert!(!port.is_closed());
        let first = event(&port, boundary);
        assert_eq!(first.dropped_total(), dropping.then_some(0));
        drop(first);
        port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), boundary);
        let expired = boundary + Duration::from_millis(5) + Duration::from_nanos(1);
        port.expire(expired).unwrap();
        if dropping {
            assert!(poll(&port, expired).is_pending());
            port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), expired);
            assert_eq!(event(&port, expired).dropped_total(), Some(1));
        } else {
            assert!(matches!(poll(&port, boundary), Poll::Ready(Some(Err(_)))));
        }
    }
}

#[test]
fn source_admission_is_not_released_while_an_executing_input_is_owned() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let admission = Admission::new(1);
    let first = port(limits(1, 4096, 1000), &budget, false);
    first.admit(&admission).unwrap();
    let now = Instant::now();
    let batch = capture(&store, &fixture, "one");
    first.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    let running = event(&first, now);
    first.close();
    drop(first);
    let second = port(limits(1, 4096, 1000), &budget, false);
    assert_eq!(
        second.admit(&admission),
        Err(super::super::PublicationError::BindingCapacity)
    );
    drop(running);
    second.admit(&admission).unwrap();
}

#[test]
fn shared_pressure_can_evict_only_owned_waiting_items_and_never_a_peer() {
    let fixture = fixture();
    let store = world(&fixture);
    let (policy, budget) = store.source_policy();
    let dropping = port(limits(2, 4096, 1000), &budget, true);
    let peer = port(limits(2, 4096, 1000), &budget, false);
    let now = Instant::now();
    let batch = capture(&store, &fixture, "one");
    dropping.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    peer.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    let full = budget
        .reserve(policy.max_retained_bytes() - budget.used(), usize::MAX)
        .unwrap();
    // Evicting this queue's one retained envelope makes room for its replacement.
    dropping.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    assert!(!dropping.is_closed());
    assert_eq!(event(&dropping, now).dropped_total(), Some(1));
    // The unrelated peer still owns exactly its original unprocessed event.
    assert_eq!(event(&peer, now).dropped_total(), None);
    assert!(poll(&peer, now).is_pending());
    drop(full);
    drop(dropping);
    drop(peer);
    drop(batch);
    assert_eq!(budget.used(), 0);
}

#[test]
fn unrepairable_shared_pressure_stops_only_the_new_recipient() {
    let fixture = fixture();
    let store = world(&fixture);
    let (policy, budget) = store.source_policy();
    let healthy = port(limits(2, 4096, 1000), &budget, false);
    let full = port(limits(2, 4096, 1000), &budget, true);
    let now = Instant::now();
    let batch = capture(&store, &fixture, "one");
    healthy.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    let pin = budget
        .reserve(policy.max_retained_bytes() - budget.used(), usize::MAX)
        .unwrap();
    full.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    assert!(full.is_closed());
    assert_eq!(full.drop_count_for_test(), 0);
    let running = event(&healthy, now);
    assert!(!running.cancellation().unwrap().cancelled());
    assert_eq!(running.dropped_total(), None);
    drop(pin);
    drop(running);
    drop(full);
    drop(healthy);
    drop(batch);
    assert_eq!(budget.used(), 0);
}

#[test]
fn eviction_counter_exhaustion_terminates_before_inexact_increment() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let port = port(limits(1, 4096, 1000), &budget, true);
    let now = Instant::now();
    let batch = capture(&store, &fixture, "one");
    port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    port.set_drop_count_for_test(super::super::MAX_SAFE_SEQUENCE);
    port.offer_at(RetainedBatch::event(&batch, 0).unwrap(), now);
    assert!(port.is_closed());
    assert_eq!(port.drop_count_for_test(), super::super::MAX_SAFE_SEQUENCE);
    assert!(matches!(poll(&port, now), Poll::Ready(Some(Err(_)))));
    assert!(matches!(poll(&port, now), Poll::Ready(None)));
}

#[test]
fn source_delivery_metadata_keeps_its_global_charge_through_the_last_reader() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let port = port(limits(1, 4096, 1000), &budget, false);
    let counter = port.source_delivery();
    port.close();
    drop(port);
    assert!(budget.used() > 0);
    assert!(counter.is_stopped());
    drop(counter);
    assert_eq!(budget.used(), 0);
}

#[test]
fn input_accounting_and_cancellation_metadata_stay_charged_after_the_port_leaves() {
    let fixture = fixture();
    let store = world(&fixture);
    let (_, budget) = store.source_policy();
    let port = port(limits(1, 4096, 1000), &budget, false);
    let input = port.input_usage();
    port.close();
    drop(port);
    assert!(budget.used() > 0);
    assert!(!input.retains_input());
    assert!(input.cancel.cancelled());
    drop(input);
    assert_eq!(budget.used(), 0);
}
