//! Every retired native join is visited before sleeping without another wake.

mod support;

use super::*;
use support::with_fixture;

#[test]
fn ready_generations_drain_without_an_unrelated_process_notification() {
    with_fixture(|fixture| {
        let registration = fixture.registration();
        fixture.ready(&registration);
        fixture.ready(&registration);
        registration.close();
        let observed = lock_process(&fixture.shared).epoch;
        poll_registrations(&fixture.shared);
        let unchanged_epoch = lock_process(&fixture.shared).epoch == observed;
        let drain = registration.poll_drained(&mut Context::from_waker(Waker::noop()));
        assert!(
            registration.drained(),
            "ready joins were left unvisited before the epoch wait: unchanged_epoch={}, states={:?}",
            unchanged_epoch,
            drain
        );
    });
}

#[test]
fn a_pending_generation_cannot_hide_a_ready_generation_in_the_same_registration() {
    with_fixture(|fixture| {
        let registration = fixture.registration();
        fixture.pending(&registration);
        fixture.ready(&registration);
        registration.close();
        let observed = lock_process(&fixture.shared).epoch;
        poll_registrations(&fixture.shared);
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(registration.poll_drained(&mut context), Poll::Pending);
        // The pending native handle keeps its slot; the ready second handle
        // must have actually joined before an unchanged-epoch sleep is allowed.
        assert_eq!(
            fixture.shared.state.lock().unwrap().epoch,
            observed,
            "fixture polling must not manufacture another wake"
        );
        assert_eq!(
            registration.retained_states_for_test(),
            [0, 0, 1, 0, 0, 0, 0]
        );
    });
}

#[test]
fn all_pending_generations_leave_the_reaper_ready_to_sleep() {
    with_fixture(|fixture| {
        let registration = fixture.registration();
        fixture.pending(&registration);
        fixture.pending(&registration);
        registration.close();
        let observed = lock_process(&fixture.shared).epoch;
        poll_registrations(&fixture.shared);
        assert_eq!(lock_process(&fixture.shared).epoch, observed);
        assert_eq!(
            registration.poll_drained(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        );
    });
}

#[test]
fn pending_records_in_one_endpoint_do_not_hide_another_endpoint_ready_records() {
    with_fixture(|fixture| {
        let first = fixture.registration();
        let second = fixture.registration();
        fixture.pending(&first);
        fixture.pending(&first);
        fixture.ready(&second);
        fixture.ready(&second);
        first.close();
        second.close();
        poll_registrations(&fixture.shared);
        assert!(second.drained());
        assert_eq!(
            first.poll_drained(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        );
    });
}
