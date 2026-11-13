use super::super::{
    ByteReservationError, DeliveryResources, EndpointByteOwner, EventQueue, MonotonicInstant,
    QueuedEvent,
};
use super::settings;

#[test]
fn built_endpoint_clones_share_bytes_but_separate_builds_do_not() {
    let first = DeliveryResources::new(&settings()).unwrap();
    let clone = first.clone();
    let separate = DeliveryResources::new(&settings()).unwrap();
    let first_bytes = first.byte_owner();
    let clone_bytes = clone.byte_owner();
    let separate_bytes = separate.byte_owner();

    let charge = first_bytes.try_reserve(7).unwrap();
    assert_eq!(clone_bytes.used(), 7);
    assert_eq!(separate_bytes.used(), 0);
    drop(charge);
    assert_eq!(clone_bytes.used(), 0);
}

#[test]
fn byte_charge_shrinks_moves_and_releases_exactly_once() {
    let owner = EndpointByteOwner::new(10);
    let clone = owner.clone();
    let reservation = owner.try_reserve(8).unwrap();
    assert_eq!(reservation.bytes(), 8);
    assert_eq!(clone.used(), 8);
    assert!(matches!(
        clone.try_reserve(3),
        Err(ByteReservationError::Capacity)
    ));

    let queued = reservation.commit(5).unwrap();
    assert_eq!(queued.bytes(), 5);
    assert_eq!(owner.used(), 5);
    let in_flight = queued.into_in_flight();
    assert_eq!(in_flight.bytes(), 5);
    assert_eq!(owner.used(), 5);
    drop(in_flight);
    assert_eq!(owner.used(), 0);
    assert!(!owner.is_corrupted());

    let independent = EndpointByteOwner::new(10);
    let _separate = independent.try_reserve(10).unwrap();
    assert_eq!(owner.used(), 0);
}

#[test]
fn failed_shrink_releases_the_original_reservation() {
    let owner = EndpointByteOwner::new(8);
    let reservation = owner.try_reserve(8).unwrap();
    assert!(matches!(
        reservation.commit(9),
        Err(ByteReservationError::InvalidShrink)
    ));
    assert_eq!(owner.used(), 0);
}

#[test]
fn recoverable_shrink_retains_the_original_token_until_success() {
    let owner = EndpointByteOwner::new(8);
    let mut reservation = owner.try_reserve(8).unwrap();
    assert!(matches!(
        reservation.try_commit(9),
        Err(ByteReservationError::InvalidShrink)
    ));
    assert_eq!(owner.used(), 8);
    assert_eq!(reservation.bytes(), 8);
    let exact = reservation.try_commit(3).unwrap();
    assert_eq!(owner.used(), 3);
    assert_eq!(reservation.bytes(), 0);
    drop(reservation);
    assert_eq!(owner.used(), 3);
    drop(exact);
    assert_eq!(owner.used(), 0);
    assert!(!owner.is_corrupted());
}

#[test]
fn an_existing_reservation_cannot_commit_after_shared_owner_corruption() {
    let owner = EndpointByteOwner::new(8);
    let mut reservation = owner.try_reserve(8).unwrap();
    owner.clone().corrupt_for_test();
    assert!(matches!(
        reservation.try_commit(3),
        Err(ByteReservationError::Accounting)
    ));
    assert_eq!(reservation.bytes(), 8);
    assert_eq!(owner.used(), 8);
    assert!(matches!(
        owner.try_reserve(0),
        Err(ByteReservationError::Accounting)
    ));
    drop(reservation);
    assert_eq!(owner.used(), 0);
    assert!(owner.is_corrupted());
}

#[test]
fn queue_evicts_only_whole_fifo_entries_and_inflight_stays_charged() {
    let owner = EndpointByteOwner::new(20);
    let first = owner.try_reserve(8).unwrap().commit(5).unwrap();
    let second = owner.try_reserve(8).unwrap().commit(7).unwrap();
    let mut queue = EventQueue::new();
    queue
        .push(QueuedEvent::new(
            MonotonicInstant::from_nanos(10),
            "first",
            first,
        ))
        .unwrap();
    queue
        .push(QueuedEvent::new(
            MonotonicInstant::from_nanos(20),
            "second",
            second,
        ))
        .unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.bytes(), 12);
    assert_eq!(
        queue.oldest_admitted_at(),
        Some(MonotonicInstant::from_nanos(10))
    );

    let evicted = queue.evict_oldest().unwrap().unwrap();
    drop(evicted);
    assert_eq!(owner.used(), 7);

    let event = queue.take_oldest().unwrap().unwrap();
    let (slot, mut in_flight) = event
        .into_in_flight_with(MonotonicInstant::from_nanos(30), 0, |payload, _| {
            Ok::<_, ()>(payload)
        })
        .unwrap();
    assert!(queue.is_empty());
    assert_eq!(queue.bytes(), 0);
    assert_eq!(in_flight.payload(), &"second");
    let mut progress = in_flight.take_progress().unwrap();
    assert_eq!(
        progress.accept(0, MonotonicInstant::from_nanos(40)),
        Ok(false)
    );
    assert_eq!(
        progress.last_progress(),
        Ok(MonotonicInstant::from_nanos(30))
    );
    assert_eq!(
        progress.accept(7, MonotonicInstant::from_nanos(41)),
        Ok(true)
    );
    assert_eq!(
        progress.last_progress(),
        Ok(MonotonicInstant::from_nanos(41))
    );
    assert_eq!(owner.used(), 7);
    drop((slot, in_flight, progress));
    assert_eq!(owner.used(), 0);
}
