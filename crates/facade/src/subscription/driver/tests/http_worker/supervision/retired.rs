use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use super::super::super::state::{SourceStep, State, count};
use super::super::support::{profiles, runtime};
use super::aggregate_support::poll_driver_once;
use super::support::{
    active, assert_complete, assert_safe_error, attach_with_sequenced_event_factory, close,
    drive_for, drive_until, next_frame, next_unaccepted_frame, poll_body_once,
};
use crate::subscription::outbox::SubscriptionOutboxSignal;
use crate::transport::streaming::supervisor::HttpStreamingError;

#[test]
fn unaccepted_event_keeps_its_original_send_deadline_after_worker_timeout() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(1), SourceStep::Event(2)]);
        let (mut live, signals) =
            attach_with_sequenced_event_factory(&runtime, Arc::clone(&probe), profile, true);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        drop(initial);
        let held = next_unaccepted_frame(&runtime, &mut live)
            .unwrap()
            .expect("first event remains unaccepted");
        assert_eq!(count(&probe.counts.nested_calls), 1);
        assert_eq!(active(&live), 1);
        assert!(bytes.used() > 0);

        // The first frame owns the deadline at 100ms. Retire the timed-out worker at
        // delivery time 50ms so resetting that deadline would incorrectly postpone it to 150ms.
        live.driver
            .as_mut()
            .get_mut()
            .set_clock_for_test(50_000_000)
            .unwrap();
        drive_until(&runtime, &mut live.driver, || {
            count(&probe.counts.factory_future_drops) == 1 && signals.completed().len() == 1
        });
        live.driver
            .as_mut()
            .get_mut()
            .set_clock_for_test(99_000_000)
            .unwrap();
        assert!(matches!(
            poll_driver_once(&runtime, &mut live.driver),
            Poll::Pending
        ));
        live.driver
            .as_mut()
            .get_mut()
            .set_clock_for_test(100_000_000)
            .unwrap();

        let error = match next_unaccepted_frame(&runtime, &mut live) {
            Err(error) => error,
            Ok(_) => panic!("unaccepted event must reach its original send deadline"),
        };
        assert_eq!(
            error,
            HttpStreamingError::Delivery(SubscriptionOutboxSignal::SendStalled)
        );
        assert_eq!(count(&probe.counts.factory_future_drops), 1);
        assert_eq!(signals.completed().len(), 1);

        drop(held);
        assert!(matches!(
            poll_body_once(&runtime, &mut live.body),
            Poll::Ready(None)
        ));
        close(&runtime, live);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn fully_accepted_event_owner_does_not_stall_a_pending_safe_timeout() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(1), SourceStep::Event(2)]);
        let (mut live, signals) =
            attach_with_sequenced_event_factory(&runtime, Arc::clone(&probe), profile, false);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        drop(initial);
        let held = next_frame(&runtime, &mut live).expect("fully accepted first event");
        drive_until(&runtime, &mut live.driver, || {
            count(&probe.counts.factory_future_drops) == 1
        });
        assert_eq!(active(&live), 1);
        assert!(bytes.used() >= held.as_ref().len());

        drive_for(&runtime, &mut live.driver, Duration::from_millis(150));
        assert!(matches!(
            poll_body_once(&runtime, &mut live.body),
            Poll::Pending
        ));

        drop(held);
        let error = next_frame(&runtime, &mut live).expect("safe event timeout");
        assert_safe_error(profile, error.as_ref(), "REQUEST_TIMEOUT");
        drop(error);
        let complete = next_frame(&runtime, &mut live).expect("timeout completion");
        assert_complete(profile, complete.as_ref());
        drop(complete);
        assert!(next_frame(&runtime, &mut live).is_none());
        assert_eq!(count(&probe.counts.factory_future_drops), 1);
        assert_eq!(signals.completed().len(), 1);

        close(&runtime, live);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(bytes.used(), 0);
    }
}
