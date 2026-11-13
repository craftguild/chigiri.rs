//! Move-only staging reservations and fixed queue saturation.

use super::super::PublicationError;
use super::super::limits::{STAGING_BATCHES, STAGING_BYTES};
use super::super::staging::DispatchStaging;
use super::super::topics::TopicIndex;
use super::support::{batch, fixture, scalar};

#[test]
fn byte_reservation_accepts_exact_limit_and_rejects_one_over_before_growth() {
    let topics = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let exact = staging.try_reserve(STAGING_BYTES - 16).unwrap();
    assert_eq!(exact.bytes(), STAGING_BYTES);
    assert_eq!(staging.usage(), (1, STAGING_BYTES));
    assert_eq!(
        staging.try_reserve(0).err(),
        Some(PublicationError::StagingCapacity)
    );
    assert_eq!(staging.usage(), (1, STAGING_BYTES));
    drop(exact);
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn reservation_arithmetic_overflow_fails_before_usage_changes() {
    let topics = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    assert_eq!(
        staging.try_reserve(usize::MAX).err(),
        Some(PublicationError::ArithmeticOverflow)
    );
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn slot_reservations_are_move_only_and_drop_releases_exactly_once() {
    let topics = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let reservations = (0..STAGING_BATCHES)
        .map(|_| staging.try_reserve(0).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(staging.usage(), (STAGING_BATCHES, STAGING_BATCHES * 16));
    assert_eq!(
        staging.try_reserve(0).err(),
        Some(PublicationError::StagingCapacity)
    );
    drop(reservations);
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn foreign_stage_returns_all_original_owners_without_mutation() {
    let fixture = fixture();
    let captured = batch(&fixture, "foreign", &scalar(1_i64));
    let bytes = captured.bytes();
    let mut first_topics = TopicIndex::new(1).unwrap();
    let second_topics = TopicIndex::new(1).unwrap();
    let first = DispatchStaging::new(&first_topics).unwrap();
    let second = DispatchStaging::new(&second_topics).unwrap();
    let reservation = first.try_reserve(bytes).unwrap();
    let sequence = first_topics.commit_sequence().unwrap();
    let failure = second.stage(reservation, captured, sequence).err().unwrap();
    assert_eq!(failure.error, PublicationError::ForeignOwner);
    assert_eq!(failure.batch.bytes(), bytes);
    drop(failure.reservation);
    drop(failure.batch);
    drop(failure.sequence);
    assert_eq!(first.usage(), (0, 0));
    assert_eq!(second.usage(), (0, 0));
}

#[test]
fn stage_rejects_a_foreign_commit_sequence_and_returns_all_owners() {
    let fixture = fixture();
    let captured = batch(&fixture, "foreign", &scalar(1_i64));
    let bytes = captured.bytes();
    let owner = TopicIndex::new(1).unwrap();
    let mut foreign = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&owner).unwrap();
    let reservation = staging.try_reserve(bytes).unwrap();
    let sequence = foreign.commit_sequence().unwrap();
    let failure = staging
        .stage(reservation, captured, sequence)
        .err()
        .unwrap();
    assert_eq!(failure.error, PublicationError::ForeignOwner);
    drop(failure.reservation);
    drop(failure.batch);
    drop(failure.sequence);
    assert_eq!(staging.usage(), (0, 0));
}

#[test]
fn stage_rejects_a_mismatched_byte_reservation_before_queue_mutation() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let captured = batch(&fixture, "mismatch", &scalar("longer"));
    let reservation = staging.try_reserve(captured.bytes() - 1).unwrap();
    let sequence = topics.commit_sequence().unwrap();
    let failure = staging
        .stage(reservation, captured, sequence)
        .err()
        .unwrap();
    assert_eq!(failure.error, PublicationError::Invariant);
    drop(failure.reservation);
    drop(failure.batch);
    drop(failure.sequence);
    assert_eq!(staging.usage(), (0, 0));
    assert!(staging.take().unwrap().is_none());
}

#[test]
fn stage_take_and_close_keep_charge_until_the_last_physical_owner_drops() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let captured = batch(&fixture, "ready", &scalar(true));
    let bytes = captured.bytes();
    let reservation = staging.try_reserve(bytes).unwrap();
    let sequence = topics.commit_sequence().unwrap();
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    assert_eq!(staging.usage(), (1, bytes + 16));
    let staged = staging.take().unwrap().unwrap();
    assert_eq!(staged.batch().bytes(), bytes);
    assert_eq!(staging.usage(), (1, bytes + 16));
    drop(staged);
    assert_eq!(staging.usage(), (0, 0));

    let captured = batch(&fixture, "close", &scalar(false));
    let bytes = captured.bytes();
    let reservation = staging.try_reserve(bytes).unwrap();
    let sequence = topics.commit_sequence().unwrap();
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    staging.close().unwrap();
    assert_eq!(staging.usage(), (0, 0));
    assert_eq!(
        staging.try_reserve(0).err(),
        Some(PublicationError::StagingClosed)
    );
}

#[test]
fn dropping_staging_with_a_queued_batch_has_no_owner_cycle() {
    let fixture = fixture();
    let mut topics = TopicIndex::new(1).unwrap();
    let staging = DispatchStaging::new(&topics).unwrap();
    let liveness = staging.liveness();
    let captured = batch(&fixture, "queued", &scalar(true));
    let reservation = staging.try_reserve(captured.bytes()).unwrap();
    let sequence = topics.commit_sequence().unwrap();
    assert!(staging.stage(reservation, captured, sequence).is_ok());
    assert!(liveness.any_owner_alive());
    drop(staging);
    assert!(!liveness.any_owner_alive());
}
