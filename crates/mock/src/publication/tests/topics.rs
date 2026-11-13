//! Checked topic registrations, generations, loss markers, and reset behavior.

use super::super::PublicationError;
use super::super::limits::MAX_ACTIVE_SUBSCRIPTIONS;
use super::super::topics::TopicIndex;
use super::support::{Fixture, batch, fixture, scalar};
use crate::ir::FieldId;

fn register(
    index: &mut TopicIndex,
    fixture: &Fixture,
    topic: &str,
    field: FieldId,
) -> super::super::topics::RegisteredBinding {
    let prepared = index.prepare_binding(&fixture.ir, field, topic).unwrap();
    let outcome = index.register(prepared).ok().unwrap();
    drop(outcome.retired_topic_candidate);
    outcome.binding
}

#[test]
fn registration_uses_exact_bound_and_distinct_generation_for_reused_topic() {
    let fixture = fixture();
    assert!(TopicIndex::new(1).is_ok());
    assert!(TopicIndex::new(MAX_ACTIVE_SUBSCRIPTIONS).is_ok());
    assert_eq!(
        TopicIndex::new(0).err(),
        Some(PublicationError::InvalidConfiguration)
    );
    assert_eq!(
        TopicIndex::new(MAX_ACTIVE_SUBSCRIPTIONS + 1).err(),
        Some(PublicationError::InvalidConfiguration)
    );

    let mut index = TopicIndex::new(2).unwrap();
    let first = register(&mut index, &fixture, "shared", fixture.event);
    let first_generation = first.generation();
    let first_registration = first.registration_sequence();
    assert_eq!(index.counts(), (1, 1));
    assert_eq!(
        index.binding_charge(first_generation),
        Some(64 + 2 * ("shared".len() + "Subscription.event".len()))
    );
    drop(index.unregister(first).ok().unwrap());
    assert_eq!(index.counts(), (0, 0));

    let second = register(&mut index, &fixture, "shared", fixture.event);
    assert!(second.generation() > first_generation);
    assert!(second.registration_sequence() > first_registration);
    drop(index.unregister(second).ok().unwrap());
    assert_eq!(index.counts(), (0, 0));
}

#[test]
fn duplicate_topic_keeps_one_topic_until_last_binding_and_rejects_foreign_owner() {
    let fixture = fixture();
    let mut first = TopicIndex::new(2).unwrap();
    let one = register(&mut first, &fixture, "topic", fixture.event);
    let two = register(&mut first, &fixture, "topic", fixture.event);
    assert_eq!(first.counts(), (1, 2));
    drop(first.unregister(one).ok().unwrap());
    assert_eq!(first.counts(), (1, 1));

    let mut foreign = TopicIndex::new(1).unwrap();
    let prepared = first
        .prepare_binding(&fixture.ir, fixture.event, "foreign")
        .unwrap();
    let failure = foreign.register(prepared).err().unwrap();
    assert_eq!(failure.error, PublicationError::ForeignOwner);
    drop(failure.prepared);
    let failure = foreign.unregister(two).err().unwrap();
    assert_eq!(failure.0, PublicationError::ForeignOwner);
    drop(failure.1);
    assert_eq!((first.counts(), foreign.counts()), ((1, 1), (0, 0)));
}

#[test]
fn topic_and_coordinate_validation_and_capacity_fail_before_registration() {
    let fixture = fixture();
    let index = TopicIndex::new(1).unwrap();
    let over = "x".repeat(257);
    for invalid in ["", "line\nbreak", over.as_str()] {
        assert_eq!(
            index
                .prepare_binding(&fixture.ir, fixture.event, invalid)
                .err(),
            Some(PublicationError::InvalidTopic)
        );
    }
    assert!(
        index
            .prepare_binding(&fixture.ir, fixture.event, &"x".repeat(256))
            .is_ok()
    );
    assert_eq!(
        index
            .prepare_binding(&fixture.ir, fixture.query_unit, "topic")
            .err(),
        Some(PublicationError::InvalidCoordinate)
    );

    let mut index = TopicIndex::new(1).unwrap();
    let binding = register(&mut index, &fixture, "topic", fixture.event);
    let prepared = index
        .prepare_binding(&fixture.ir, fixture.event, "other")
        .unwrap();
    let failure = index.register(prepared).err().unwrap();
    assert_eq!(failure.error, PublicationError::BindingCapacity);
    drop(failure.prepared);
    assert_eq!(index.counts(), (1, 1));
    drop(index.unregister(binding).ok().unwrap());
}

#[test]
fn loss_sequence_affects_only_registrations_before_that_commit() {
    let fixture = fixture();
    let captured = batch(&fixture, "events", &scalar(true));
    let mut index = TopicIndex::new(3).unwrap();
    let first = register(&mut index, &fixture, "events", fixture.event);
    let first_registration = first.registration_sequence();
    let lost = index.commit_sequence().unwrap();
    index.raise_loss(&lost, &captured).unwrap();
    assert!(
        index
            .next_binding("events", "Subscription.event", 0, lost.value())
            .unwrap()
            .lost
    );

    let second = register(&mut index, &fixture, "events", fixture.event);
    let current = index.commit_sequence().unwrap();
    let second_target = index
        .next_binding(
            "events",
            "Subscription.event",
            first_registration,
            current.value(),
        )
        .unwrap();
    assert_eq!(second_target.generation, second.generation());
    assert!(!second_target.lost);
    index.raise_loss(&current, &captured).unwrap();
    assert!(
        index
            .next_binding(
                "events",
                "Subscription.event",
                first_registration,
                current.value() + 1,
            )
            .unwrap()
            .lost
    );
}

#[test]
fn loss_rejects_a_commit_sequence_from_another_server() {
    let fixture = fixture();
    let captured = batch(&fixture, "events", &scalar(true));
    let mut first = TopicIndex::new(1).unwrap();
    let mut second = TopicIndex::new(1).unwrap();
    let sequence = first.commit_sequence().unwrap();
    assert_eq!(
        second.raise_loss(&sequence, &captured),
        Err(PublicationError::ForeignOwner)
    );
}

#[test]
fn last_binding_removal_recreates_topic_with_a_fresh_loss_marker() {
    let fixture = fixture();
    let captured = batch(&fixture, "events", &scalar(true));
    let mut index = TopicIndex::new(1).unwrap();
    let old = register(&mut index, &fixture, "events", fixture.event);
    let lost = index.commit_sequence().unwrap();
    index.raise_loss(&lost, &captured).unwrap();
    drop(index.unregister(old).ok().unwrap());
    assert_eq!(index.counts(), (0, 0));

    let fresh = register(&mut index, &fixture, "events", fixture.event);
    let next = index.commit_sequence().unwrap();
    let target = index
        .next_binding("events", "Subscription.event", 0, next.value())
        .unwrap();
    assert_eq!(target.generation, fresh.generation());
    assert!(!target.lost);
}

#[test]
fn shared_sequence_exhaustion_and_shutdown_reset_fail_closed() {
    let fixture = fixture();
    let mut index = TopicIndex::with_test_sequence_limit(2, 2).unwrap();
    let binding = register(&mut index, &fixture, "topic", fixture.event);
    let commit = index.commit_sequence().unwrap();
    assert_eq!((binding.registration_sequence(), commit.value()), (1, 2));
    let prepared = index
        .prepare_binding(&fixture.ir, fixture.event, "other")
        .unwrap();
    let failure = index.register(prepared).err().unwrap();
    assert_eq!(failure.error, PublicationError::SequenceExhausted);
    drop(failure.prepared);
    drop(index.shutdown());
    assert_eq!(index.counts(), (0, 0));
    assert_eq!(
        index
            .prepare_binding(&fixture.ir, fixture.event, "topic")
            .err(),
        Some(PublicationError::StagingClosed)
    );
    assert_eq!(
        index.commit_sequence().err(),
        Some(PublicationError::StagingClosed)
    );

    let mut restarted = TopicIndex::with_test_sequence_limit(1, 2).unwrap();
    let fresh = register(&mut restarted, &fixture, "topic", fixture.event);
    assert_eq!((fresh.generation(), fresh.registration_sequence()), (1, 1));
}
