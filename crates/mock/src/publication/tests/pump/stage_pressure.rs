//! Recover actual retained staging capacity after all dispatch cursors advanced.

use super::*;
use crate::publication::limits::STAGING_BATCHES;

fn fill(world: &Arc<Mutex<PublicationStore>>, fixture: &Fixture) -> PublicationPump {
    let mut pump = PublicationPump::new();
    for index in 0..STAGING_BATCHES {
        stage(world, batch(fixture, "events", &[&index.to_string()]));
        drive(&mut pump, world, 8);
    }
    let world = world.lock().unwrap();
    assert!(world.staging.front().unwrap().is_none());
    assert_eq!(world.staging.usage().0, STAGING_BATCHES);
    pump
}

fn rejected(
    world: &Arc<Mutex<PublicationStore>>,
    fixture: &Fixture,
) -> crate::publication::PublicationBuffer {
    let world = world.lock().unwrap();
    let buffer = world
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap();
    buffer
        .push(&fixture.ir, fixture.event, "events", &scalar("next"))
        .unwrap();
    assert_eq!(
        buffer.prepare(&world.staging).err(),
        Some(PublicationError::StagingCapacity)
    );
    buffer
}

#[test]
fn pressure_terminates_a_real_retainer_after_cursor_reclamation_and_preserves_idle_peer() {
    let fixture = fixture();
    let world = world(&fixture, 2);
    let mut source = register(&world, &fixture, "events");
    let mut peer = register(&world, &fixture, "other");
    let mut pump = fill(&world, &fixture);
    let rejected = rejected(&world, &fixture);
    drive(&mut pump, &world, 12);
    assert!(matches!(poll(&mut source), Poll::Ready(Some(Err(_)))));
    assert!(poll(&mut peer).is_pending());
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
    assert_eq!(
        rejected.prepare(&world.lock().unwrap().staging).err(),
        Some(PublicationError::StagingClosed)
    );
    stage(&world, batch(&fixture, "other", &["recovered"]));
    drive(&mut pump, &world, 8);
    assert_eq!(
        event(&mut peer).publication().payload(),
        &CapturedValue::String("recovered".into())
    );
}

#[test]
fn drop_oldest_releases_only_the_one_waiting_batch_needed_for_stage_recovery() {
    let fixture = fixture();
    let world = world(&fixture, 1);
    let prepared = world
        .lock()
        .unwrap()
        .registration_owner()
        .prepare(&fixture.ir, fixture.event, "events")
        .unwrap();
    let registered = world
        .lock()
        .unwrap()
        .register_source(prepared, SubscriptionOverflow::DropOldest)
        .ok()
        .unwrap()
        .finish();
    let delivery = registered.source_delivery();
    let (mut source, _) = registered.into_parts();
    let mut pump = fill(&world, &fixture);
    let rejected = rejected(&world, &fixture);
    drive(&mut pump, &world, 12);
    assert_eq!(world.lock().unwrap().staging.usage().0, STAGING_BATCHES - 1);
    assert!(!delivery.is_stopped());
    let first = event(&mut source);
    assert_eq!(
        first.publication().payload(),
        &CapturedValue::String("1".into())
    );
    assert_eq!(first.dropped_total(), Some(1));
    assert_eq!(
        rejected.prepare(&world.lock().unwrap().staging).err(),
        Some(PublicationError::StagingClosed)
    );
    drop(source);
    drive(&mut pump, &world, 4);
    assert_eq!(world.lock().unwrap().staging.usage().0, 1);
    drop(first);
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
}

#[test]
fn an_impossible_single_reservation_cannot_stop_an_existing_valid_source() {
    let fixture = fixture();
    let world = world(&fixture, 1);
    let mut source = register(&world, &fixture, "events");
    stage(&world, batch(&fixture, "events", &["valid"]));
    let mut pump = PublicationPump::new();
    drive(&mut pump, &world, 8);
    assert_eq!(
        world
            .lock()
            .unwrap()
            .staging
            .try_reserve(STAGING_BYTES)
            .err(),
        Some(PublicationError::StagingCapacity)
    );
    drive(&mut pump, &world, 8);
    assert_eq!(
        event(&mut source).publication().payload(),
        &CapturedValue::String("valid".into())
    );
    assert_eq!(world.lock().unwrap().staging.usage(), (0, 0));
}
