//! Exercise precommit capacity and store-win loss semantics against real candidates.

use super::super::capture::CapturedValue;
use super::super::limits::STAGING_BATCHES;
use super::super::{PublicationBuffer, PublicationError, PublicationLimits, PublicationStore};
use super::support::{Fixture, fixture, record_output, scalar};
use crate::store::{CallScope, RequestPrepared};
use crate::value::ValueBudget;
use chigiri::core::{Object, Value};

fn world(fixture: &Fixture) -> PublicationStore {
    let (store, _) = record_output(fixture, "original");
    PublicationStore::new(store, 4).unwrap()
}

fn changed(world: &PublicationStore, fixture: &Fixture, name: &str) -> RequestPrepared {
    let mut request = world.begin_request(ValueBudget::default());
    let mut root = request.store_mut().begin(CallScope::MutationRoot).unwrap();
    let mut selection = Object::new();
    selection.insert("field", "id");
    selection.insert("op", "eq");
    selection.insert("value", "u");
    let mut options = Object::new();
    options.insert("where", selection);
    let mut patch = Object::new();
    patch.insert("name", name);
    root.update(fixture.unit, &options, &patch).unwrap();
    request.store_mut().commit(root.prepare().unwrap()).unwrap();
    request.prepare().unwrap()
}

fn buffer(world: &PublicationStore, fixture: &Fixture, topics: &[&str]) -> PublicationBuffer {
    let buffer = world
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap();
    for topic in topics {
        buffer
            .push(&fixture.ir, fixture.event, topic, &scalar(*topic))
            .unwrap();
    }
    buffer
}

fn name(world: &PublicationStore, fixture: &Fixture) -> Value {
    let name = fixture.ir.models()[fixture.unit.0].slots["name"];
    world
        .snapshot()
        .record(fixture.unit, "u")
        .unwrap()
        .get(name)
        .unwrap()
        .clone()
}

#[test]
fn request_capture_closes_once_and_transfer_moves_the_original_reserved_allocation() {
    let fixture = fixture();
    let mut world = world(&fixture);
    let request = changed(&world, &fixture, "committed");
    let captures = buffer(&world, &fixture, &["first", "second"]);
    let mut prepared = captures.prepare(&world.staging).unwrap();
    let original = prepared
        .as_ref()
        .unwrap()
        .retained
        .batch()
        .publications()
        .as_ptr();
    let bytes = prepared.as_ref().unwrap().retained.batch().bytes();
    assert_eq!(world.staging.usage(), (1, bytes + 16));
    assert!(world.staging.take().unwrap().is_none());
    assert_eq!(name(&world, &fixture), Value::from("original"));
    assert_eq!(
        captures.prepare(&world.staging).err(),
        Some(PublicationError::StagingClosed)
    );

    let retired = world.commit_publications(&request, &mut prepared).unwrap();
    let observations: Vec<_> = retired.observations().collect();
    assert_eq!(observations.len(), 2);
    for (index, decision) in observations.iter().enumerate() {
        assert_eq!(decision.sequence, 1);
        assert_eq!(decision.index, index);
        assert_eq!(decision.coordinate, "Subscription.event");
        assert!(!decision.lost);
    }
    assert!(prepared.is_none());
    assert_eq!(name(&world, &fixture), Value::from("committed"));
    let staged = world.staging.take().unwrap().unwrap();
    assert_eq!(staged.batch().publications().as_ptr(), original);
    assert_eq!(staged.batch().publications()[0].topic(), "first");
    assert_eq!(staged.batch().publications()[1].topic(), "second");
    assert_eq!(
        staged.batch().publications()[1].payload(),
        &CapturedValue::String("second".into())
    );
    assert_eq!(world.staging.usage(), (1, bytes + 16));
    drop(retired);
    drop(staged);
    assert_eq!(world.staging.usage(), (0, 0));
}

#[test]
fn a_failed_capture_is_sticky_across_clones_and_reserves_nothing() {
    let fixture = fixture();
    let world = world(&fixture);
    let captures = buffer(&world, &fixture, &["valid"]);
    let clone = captures.clone();
    assert_eq!(
        clone.push(&fixture.ir, fixture.event, "bad\n", &scalar(1_i64)),
        Err(PublicationError::InvalidTopic)
    );
    assert!(
        captures
            .push(&fixture.ir, fixture.event, "valid-again", &scalar(1_i64))
            .is_err()
    );
    assert_eq!(
        captures.prepare(&world.staging).err(),
        Some(PublicationError::InvalidPayload)
    );
    assert_eq!(world.staging.usage(), (0, 0));
    assert!(world.staging.take().unwrap().is_none());
}

#[test]
fn precommit_saturation_and_rollback_release_capacity_without_store_change() {
    let fixture = fixture();
    let world = world(&fixture);
    let occupied = (0..STAGING_BATCHES)
        .map(|_| world.staging.try_reserve(0).unwrap())
        .collect::<Vec<_>>();
    let captures = buffer(&world, &fixture, &["rejected"]);
    assert_eq!(
        captures.prepare(&world.staging).err(),
        Some(PublicationError::StagingCapacity)
    );
    assert_eq!(name(&world, &fixture), Value::from("original"));
    drop(occupied);
    let recovered = buffer(&world, &fixture, &["recovered"]);
    let prepared = recovered.prepare(&world.staging).unwrap();
    assert_eq!(world.staging.usage().0, 1);
    drop(prepared);
    assert_eq!(world.staging.usage(), (0, 0));
    assert!(world.staging.take().unwrap().is_none());
}

#[test]
fn stale_candidates_leave_capture_and_reservation_with_the_caller() {
    let fixture = fixture();
    let mut world = world(&fixture);
    let stale = changed(&world, &fixture, "must-not-win");
    let winner = changed(&world, &fixture, "winner");
    drop(world.commit_request(&winner).unwrap());
    let mut captures = buffer(&world, &fixture, &["stale"])
        .prepare(&world.staging)
        .unwrap();
    assert!(world.commit_publications(&stale, &mut captures).is_err());
    assert!(captures.is_some());
    assert_eq!(name(&world, &fixture), Value::from("winner"));
    assert!(world.staging.take().unwrap().is_none());
    drop(captures);
    assert_eq!(world.staging.usage(), (0, 0));
}

#[test]
fn a_post_store_transfer_failure_marks_only_eligible_bindings_and_never_rolls_back() {
    let fixture = fixture();
    let mut world = world(&fixture);
    let binding = world
        .topics
        .prepare_binding(&fixture.ir, fixture.event, "topic")
        .unwrap();
    let binding = world
        .topics
        .register(binding)
        .unwrap_or_else(|_| panic!("registration failed"))
        .binding;
    let request = changed(&world, &fixture, "store-win");
    let mut captures = buffer(&world, &fixture, &["topic"])
        .prepare(&world.staging)
        .unwrap();
    world.staging.close().unwrap();
    let retired = world.commit_publications(&request, &mut captures).unwrap();
    let observations: Vec<_> = retired.observations().collect();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].sequence, 2);
    assert_eq!(observations[0].index, 0);
    assert_eq!(observations[0].coordinate, "Subscription.event");
    assert!(observations[0].lost);
    assert!(captures.is_none());
    assert_eq!(name(&world, &fixture), Value::from("store-win"));
    assert!(world.staging.take().unwrap().is_none());
    assert_eq!(world.staging.usage().0, 1);
    let target = world
        .topics
        .next_binding("topic", "Subscription.event", 0, 100)
        .unwrap();
    assert!(target.lost);
    assert_eq!(target.generation, binding.generation());
    let late = world
        .topics
        .prepare_binding(&fixture.ir, fixture.event, "topic")
        .unwrap();
    let late = world
        .topics
        .register(late)
        .unwrap_or_else(|_| panic!("late registration failed"))
        .binding;
    let target = world
        .topics
        .next_binding(
            "topic",
            "Subscription.event",
            binding.registration_sequence(),
            100,
        )
        .unwrap();
    assert_eq!(target.generation, late.generation());
    assert!(!target.lost);
    drop(retired);
    assert_eq!(world.staging.usage(), (0, 0));
}

#[test]
fn sequence_exhaustion_rejects_mutation_before_store_exchange() {
    let fixture = fixture();
    let mut world = world(&fixture);
    world.topics = super::super::topics::TopicIndex::with_test_sequence_limit(1, 1).unwrap();
    let first = changed(&world, &fixture, "last-valid");
    drop(world.commit_request(&first).unwrap());
    assert_eq!(
        world.mutation_admission(),
        Err(PublicationError::SequenceExhausted)
    );
    let next = changed(&world, &fixture, "must-not-win");
    assert!(world.commit_request(&next).is_err());
    assert_eq!(name(&world, &fixture), Value::from("last-valid"));
}

#[test]
fn an_empty_capture_closes_without_spending_a_staging_slot() {
    let fixture = fixture();
    let world = world(&fixture);
    let empty = world
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap();
    assert!(empty.prepare(&world.staging).unwrap().is_none());
    assert_eq!(
        empty.prepare(&world.staging).err(),
        Some(PublicationError::StagingClosed)
    );
    assert_eq!(world.staging.usage(), (0, 0));
}
