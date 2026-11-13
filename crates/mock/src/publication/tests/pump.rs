//! Deterministic source demand, fairness, pressure, and retained-owner proofs.

#[path = "pump/stage_pressure.rs"]
mod stage_pressure;

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use chigiri::SubscriptionOverflow;
use futures_core::Stream;

use super::super::batch::BatchBuilder;
use super::super::capture::CapturedValue;
use super::super::limits::{MAX_PUBLICATIONS, STAGING_BYTES};
use super::super::source::PublicationSource;
use super::super::{
    CapturedEvent, PublicationError, PublicationLimits, PublicationPump, PublicationStore,
};
use super::support::{Fixture, exact_limits, fixture, scalar};
use crate::store::Store;
use crate::value::ValueBudget;

struct Noop;

impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn world(fixture: &Fixture, bindings: usize) -> Arc<Mutex<PublicationStore>> {
    Arc::new(Mutex::new(
        PublicationStore::new(Store::empty(Arc::clone(&fixture.ir)), bindings).unwrap(),
    ))
}

fn register(
    world: &Arc<Mutex<PublicationStore>>,
    fixture: &Fixture,
    topic: &str,
) -> PublicationSource {
    let owner = world.lock().unwrap().registration_owner();
    let prepared = owner.prepare(&fixture.ir, fixture.event, topic).unwrap();
    let registered = world
        .lock()
        .unwrap()
        .register_source(prepared, SubscriptionOverflow::Backpressure)
        .ok()
        .unwrap()
        .finish();
    registered.into_parts().0
}

fn batch(fixture: &Fixture, topic: &str, values: &[&str]) -> super::super::CapturedBatch {
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    for value in values {
        builder
            .push(&fixture.ir, fixture.event, topic, &scalar(*value))
            .unwrap();
    }
    builder.seal().unwrap()
}

fn stage(world: &Arc<Mutex<PublicationStore>>, captured: super::super::CapturedBatch) -> usize {
    let bytes = captured.bytes();
    let (staging, reservation, sequence) = {
        let mut world = world.lock().unwrap();
        let staging = world.staging();
        let reservation = staging.try_reserve(bytes).unwrap();
        let sequence = world.topics.commit_sequence().unwrap();
        (staging, reservation, sequence)
    };
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    bytes + 16
}

fn poll(
    source: &mut PublicationSource,
) -> Poll<Option<Result<CapturedEvent, crate::luau::ScriptError>>> {
    let waker = Waker::from(Arc::new(Noop));
    Pin::new(source).poll_next(&mut Context::from_waker(&waker))
}

fn event(source: &mut PublicationSource) -> CapturedEvent {
    let Poll::Ready(Some(Ok(event))) = poll(source) else {
        panic!("source event required")
    };
    event
}

fn drive(pump: &mut PublicationPump, world: &Arc<Mutex<PublicationStore>>, count: usize) {
    for _ in 0..count {
        let _ = pump.step(world).unwrap();
    }
}

#[test]
fn same_batch_order_fairness_and_last_event_drop_preserve_one_charge() {
    let fixture = fixture();
    let world = world(&fixture, 2);
    let mut slow = register(&world, &fixture, "events");
    let mut healthy = register(&world, &fixture, "events");
    assert!(poll(&mut slow).is_pending());
    assert!(poll(&mut healthy).is_pending());
    let charge = stage(&world, batch(&fixture, "events", &["one", "two"]));
    let mut pump = PublicationPump::new();
    drive(&mut pump, &world, 2);

    let first = event(&mut healthy);
    assert_eq!(
        first.publication().payload(),
        &CapturedValue::String("one".into())
    );
    assert!(poll(&mut healthy).is_pending());
    drive(&mut pump, &world, 2);
    let second = event(&mut healthy);
    assert_eq!(
        second.publication().payload(),
        &CapturedValue::String("two".into())
    );
    assert!(first.shares_batch_with(&second));
    assert_eq!(world.lock().unwrap().staging.usage(), (1, charge));

    drop(slow);
    drive(&mut pump, &world, 6);
    assert_eq!(world.lock().unwrap().staging.usage(), (1, charge));
    drop(first);
    assert_eq!(world.lock().unwrap().staging.usage(), (1, charge));
    drop(second);
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
}

#[test]
fn one_commit_delivers_the_hard_maximum_in_exact_order_without_a_raw_queue() {
    let fixture = fixture();
    let world = world(&fixture, 1);
    let mut source = register(&world, &fixture, "events");
    let mut builder = BatchBuilder::new(exact_limits(STAGING_BYTES)).unwrap();
    for index in 0..MAX_PUBLICATIONS {
        builder
            .push(
                &fixture.ir,
                fixture.event,
                "events",
                &scalar(index.to_string()),
            )
            .unwrap();
    }
    stage(&world, builder.seal().unwrap());
    let mut pump = PublicationPump::new();
    for index in 0..MAX_PUBLICATIONS {
        assert!(poll(&mut source).is_pending());
        assert!(pump.step(&world).unwrap());
        let event = event(&mut source);
        assert_eq!(
            event.publication().payload(),
            &CapturedValue::String(index.to_string().into())
        );
    }
    assert!(pump.step(&world).unwrap());
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
}

#[test]
fn a_slow_source_does_not_block_a_demanded_peer_across_batches() {
    let fixture = fixture();
    let world = world(&fixture, 2);
    let mut slow = register(&world, &fixture, "events");
    let mut healthy = register(&world, &fixture, "events");
    assert!(poll(&mut slow).is_pending());
    assert!(poll(&mut healthy).is_pending());
    stage(&world, batch(&fixture, "events", &["first"]));
    let mut pump = PublicationPump::new();
    drive(&mut pump, &world, 2);
    let healthy_first = event(&mut healthy);
    assert_eq!(
        healthy_first.publication().payload(),
        &CapturedValue::String("first".into())
    );
    drop(healthy_first);
    assert!(poll(&mut healthy).is_pending());
    stage(&world, batch(&fixture, "events", &["second"]));
    // Reclaim the completed first batch, scan the undemanded slow source, then
    // offer the second batch to the demanded peer.
    drive(&mut pump, &world, 3);
    let healthy_second = event(&mut healthy);
    assert_eq!(
        healthy_second.publication().payload(),
        &CapturedValue::String("second".into())
    );
    let slow_first = event(&mut slow);
    assert_eq!(
        slow_first.publication().payload(),
        &CapturedValue::String("first".into())
    );
}

#[test]
fn foreign_same_schema_event_fails_owner_check_before_projection_budget_growth() {
    let fixture = fixture();
    let first = world(&fixture, 1);
    let second = world(&fixture, 1);
    let mut source = register(&first, &fixture, "events");
    assert!(poll(&mut source).is_pending());
    stage(&first, batch(&fixture, "events", &["owned"]));
    let mut pump = PublicationPump::new();
    drive(&mut pump, &first, 1);
    let event = event(&mut source);
    let budget = ValueBudget::default();
    assert!(first.lock().unwrap().owns_event(&event));
    assert!(!second.lock().unwrap().owns_event(&event));
    assert_eq!((budget.nodes(), budget.bytes()), (0, 0));
}

#[test]
fn byte_pressure_retires_only_the_front_blocker_and_same_candidate_stays_failed() {
    let fixture = fixture();
    let world = world(&fixture, 1);
    let mut source = register(&world, &fixture, "events");
    let large = "x".repeat(STAGING_BYTES / 2);
    let first = {
        let mut builder = BatchBuilder::new(exact_limits(STAGING_BYTES)).unwrap();
        builder
            .push(
                &fixture.ir,
                fixture.event,
                "events",
                &scalar(large.as_str()),
            )
            .unwrap();
        builder.seal().unwrap()
    };
    stage(&world, first);
    assert_eq!(world.lock().unwrap().staging.usage().0, 1);

    let (staging, rejected) = {
        let world = world.lock().unwrap();
        let buffer = world
            .capture_owner()
            .buffer(exact_limits(STAGING_BYTES))
            .unwrap();
        buffer
            .push(
                &fixture.ir,
                fixture.event,
                "events",
                &scalar(large.as_str()),
            )
            .unwrap();
        (world.staging(), buffer)
    };
    assert_eq!(
        rejected.prepare(&staging).err(),
        Some(PublicationError::StagingCapacity)
    );
    let mut pump = PublicationPump::new();
    drive(&mut pump, &world, 3);
    let Poll::Ready(Some(Err(error))) = poll(&mut source) else {
        panic!("pressure terminal required")
    };
    assert_eq!(error.phase, "source");
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
    assert_eq!(
        rejected.prepare(&staging).err(),
        Some(PublicationError::StagingClosed)
    );

    let later = {
        let world = world.lock().unwrap();
        let buffer = world
            .capture_owner()
            .buffer(PublicationLimits::default())
            .unwrap();
        buffer
            .push(&fixture.ir, fixture.event, "events", &scalar("later"))
            .unwrap();
        buffer.prepare(&staging).unwrap().unwrap()
    };
    assert_eq!(world.lock().unwrap().staging.usage().0, 1);
    drop(later);
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
}

#[test]
fn loss_marker_terminates_only_the_generation_registered_before_loss() {
    let fixture = fixture();
    let world = world(&fixture, 2);
    let mut old = register(&world, &fixture, "events");
    let lost = batch(&fixture, "events", &["lost"]);
    {
        let mut world = world.lock().unwrap();
        let sequence = world.topics.commit_sequence().unwrap();
        world.topics.raise_loss(&sequence, &lost).unwrap();
    }
    let mut fresh = register(&world, &fixture, "events");
    assert!(poll(&mut old).is_pending());
    assert!(poll(&mut fresh).is_pending());
    let mut pump = PublicationPump::new();
    drive(&mut pump, &world, 2);
    let Poll::Ready(Some(Err(error))) = poll(&mut old) else {
        panic!("old generation must observe loss")
    };
    assert_eq!(error.phase, "source");
    assert!(poll(&mut fresh).is_pending());

    stage(&world, batch(&fixture, "events", &["fresh"]));
    drive(&mut pump, &world, 2);
    let delivered = event(&mut fresh);
    assert_eq!(
        delivered.publication().payload(),
        &CapturedValue::String("fresh".into())
    );
}

#[test]
fn shutdown_replaces_an_offered_event_and_releases_the_staging_owner() {
    let fixture = fixture();
    let world = world(&fixture, 1);
    let mut source = register(&world, &fixture, "events");
    assert!(poll(&mut source).is_pending());
    stage(&world, batch(&fixture, "events", &["held"]));
    let mut pump = PublicationPump::new();
    drive(&mut pump, &world, 1);
    assert_eq!(world.lock().unwrap().staging.usage().0, 1);

    assert!(pump.shutdown_step(&world).unwrap());
    assert!(!pump.shutdown_step(&world).unwrap());
    world.lock().unwrap().staging.close().unwrap();
    let Poll::Ready(Some(Err(error))) = poll(&mut source) else {
        panic!("shutdown terminal required")
    };
    assert_eq!(error.phase, "source");
    assert!(matches!(poll(&mut source), Poll::Ready(None)));
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
}

#[test]
fn source_drop_without_a_publication_retires_capacity_on_the_next_pump_step() {
    let fixture = fixture();
    let world = world(&fixture, 1);
    let source = register(&world, &fixture, "events");
    assert_eq!(world.lock().unwrap().topics.counts(), (1, 1));
    drop(source);
    let mut pump = PublicationPump::new();
    assert!(pump.step(&world).unwrap());
    assert_eq!(world.lock().unwrap().topics.counts(), (0, 0));
    drop(register(&world, &fixture, "replacement"));
}
