//! Allocation-free incremental dispatch and shutdown fairness.

use super::super::PublicationLimits;
use super::super::batch::BatchBuilder;
use super::super::dispatcher::{DispatchAction, IncrementalDispatcher};
use super::super::staging::DispatchStaging;
use super::super::topics::TopicIndex;
use super::support::{Fixture, batch, fixture, scalar};
use crate::ir::FieldId;

fn register(index: &mut TopicIndex, fixture: &Fixture, field: FieldId) -> u64 {
    let prepared = index.prepare_binding(&fixture.ir, field, "events").unwrap();
    let outcome = index.register(prepared).ok().unwrap();
    drop(outcome.retired_topic_candidate);
    outcome.binding.generation()
}

#[test]
fn dispatcher_visits_one_eligible_binding_at_a_time_in_registration_order() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(3).unwrap();
    let first = register(&mut topics, &fixture, fixture.event);
    let second = register(&mut topics, &fixture, fixture.event);
    register(&mut topics, &fixture, fixture.changed);
    let sequence = topics.commit_sequence().unwrap();
    let captured = batch(&fixture, "events", &scalar(7_i64));
    let bytes = captured.bytes();
    let staging = DispatchStaging::new(&topics).unwrap();
    let reservation = staging.try_reserve(bytes).unwrap();
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    let staged = staging.take().unwrap().unwrap();

    let mut dispatcher = IncrementalDispatcher::new();
    assert!(dispatcher.accept(staged).is_ok());
    for expected in [first, second] {
        match dispatcher.next(&topics).ok().unwrap() {
            DispatchAction::Deliver {
                publication,
                target,
            } => {
                assert_eq!(publication.topic(), "events");
                assert_eq!(target.generation, expected);
                assert!(!target.lost);
            }
            _ => panic!("one delivery required"),
        }
    }
    assert!(matches!(
        dispatcher.next(&topics).ok().unwrap(),
        DispatchAction::PublicationComplete
    ));
    let complete = match dispatcher.next(&topics).ok().unwrap() {
        DispatchAction::BatchComplete(batch) => batch,
        _ => panic!("batch completion required"),
    };
    assert!(matches!(
        dispatcher.next(&topics).ok().unwrap(),
        DispatchAction::Idle
    ));
    assert_eq!(staging.usage(), (1, bytes + 16));
    drop(complete);
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn dispatcher_reports_loss_and_preserves_publication_order() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(1).unwrap();
    let generation = register(&mut topics, &fixture, fixture.event);
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push(&fixture.ir, fixture.event, "events", &scalar("first"))
        .unwrap();
    builder
        .push(&fixture.ir, fixture.event, "events", &scalar("second"))
        .unwrap();
    let captured = builder.seal().unwrap();
    let bytes = captured.bytes();
    let sequence = topics.commit_sequence().unwrap();
    topics.raise_loss(&sequence, &captured).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let reservation = staging.try_reserve(bytes).unwrap();
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    let staged = staging.take().unwrap().unwrap();
    let mut dispatcher = IncrementalDispatcher::new();
    assert!(dispatcher.accept(staged).is_ok());

    for expected in ["first", "second"] {
        match dispatcher.next(&topics).ok().unwrap() {
            DispatchAction::Lost {
                publication,
                target,
            } => {
                assert_eq!(target.generation, generation);
                assert_eq!(
                    publication.payload(),
                    &super::super::capture::CapturedValue::String(expected.into())
                );
            }
            _ => panic!("loss action required"),
        }
        assert!(matches!(
            dispatcher.next(&topics).ok().unwrap(),
            DispatchAction::PublicationComplete
        ));
    }
    let complete = match dispatcher.next(&topics).ok().unwrap() {
        DispatchAction::BatchComplete(batch) => batch,
        _ => panic!("batch completion required"),
    };
    drop(complete);
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn busy_and_shutdown_return_the_exact_physical_batch_owner() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(1).unwrap();
    register(&mut topics, &fixture, fixture.event);
    let staging = DispatchStaging::new(&topics).unwrap();
    let mut staged = Vec::new();
    for topic in ["events", "events"] {
        let captured = batch(&fixture, topic, &scalar(true));
        let bytes = captured.bytes();
        let reservation = staging.try_reserve(bytes).unwrap();
        let sequence = topics.commit_sequence().unwrap();
        assert!(staging.stage(reservation, captured, sequence).is_ok());
        staged.push(staging.take().unwrap().unwrap());
    }
    let mut dispatcher = IncrementalDispatcher::new();
    assert!(dispatcher.ensure_idle().is_ok());
    assert!(dispatcher.accept(staged.remove(0)).is_ok());
    assert!(dispatcher.ensure_idle().is_err());
    let rejected = dispatcher.accept(staged.remove(0)).err().unwrap();
    drop(rejected);
    let retained = dispatcher.shutdown().unwrap();
    drop(retained);
    assert!(dispatcher.ensure_idle().is_ok());
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn dispatcher_rejects_a_foreign_topic_index_before_advancing_its_cursor() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(1).unwrap();
    let generation = register(&mut topics, &fixture, fixture.event);
    let captured = batch(&fixture, "events", &scalar(true));
    let bytes = captured.bytes();
    let sequence = topics.commit_sequence().unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let reservation = staging.try_reserve(bytes).unwrap();
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    let staged = staging.take().unwrap().unwrap();
    let mut dispatcher = IncrementalDispatcher::new();
    assert!(dispatcher.accept(staged).is_ok());

    let foreign = TopicIndex::new(1).unwrap();
    assert_eq!(
        dispatcher.next(&foreign).err(),
        Some(super::super::PublicationError::ForeignOwner)
    );
    match dispatcher.next(&topics).ok().unwrap() {
        DispatchAction::Deliver { target, .. } => assert_eq!(target.generation, generation),
        _ => panic!("cursor must remain on the first delivery"),
    }
    drop(dispatcher.shutdown());
    assert_eq!(staging.usage(), (0, 0));
}
