use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::super::super::state::{SourceStep, State, count};
use super::super::support::{drain, profiles, runtime};
use super::support::{LiveSupervision, active_endpoint, attach, drive_until, next_frame};

#[derive(Clone, Copy)]
enum ClosePoint {
    BeforeOffer,
    AfterOfferBeforeDemand,
}

#[test]
fn body_close_before_offer_cancels_without_emitting_or_requesting_again() {
    check_body_close_race(ClosePoint::BeforeOffer);
}

#[test]
fn body_close_after_offer_cancels_before_the_next_source_demand() {
    check_body_close_race(ClosePoint::AfterOfferBeforeDemand);
}

#[test]
fn body_close_cannot_overtake_an_atomically_committed_source_demand() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(10), SourceStep::Pending]);
        let (mut live, signals) = attach(&runtime, Arc::clone(&probe), profile);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        drop(initial);
        let LiveSupervision {
            endpoint,
            registration,
            mut driver,
            body,
            guard,
        } = live;
        let body = Arc::new(Mutex::new(Some(body)));
        let closer = Arc::new(Mutex::new(None::<JoinHandle<()>>));
        let (close_attempted, wait_close_attempted) = sync_channel(0);
        let (close_finished, wait_close_finished) = sync_channel(1);
        driver
            .as_ref()
            .get_ref()
            .before_close_for_test(move || close_attempted.send(()).unwrap());
        driver.as_ref().get_ref().after_demand_check_for_test({
            let body = Arc::clone(&body);
            let closer = Arc::clone(&closer);
            move || {
                let handle = std::thread::spawn(move || {
                    drop(body.lock().unwrap().take().expect("one live body"));
                    let _ = close_finished.send(());
                });
                wait_close_attempted
                    .recv()
                    .expect("closer reached the held timeline");
                assert_eq!(wait_close_finished.try_recv(), Err(TryRecvError::Empty));
                *closer.lock().unwrap() = Some(handle);
            }
        });

        drive_until(&runtime, &mut driver, || {
            count(&probe.counts.source_drops) == 1
        });
        closer
            .lock()
            .unwrap()
            .take()
            .expect("one closer")
            .join()
            .unwrap();
        // The port commit linearizes first. The worker may observe that demand
        // or cancellation first, but no third application poll may escape.
        assert!((1..=2).contains(&count(&probe.counts.source_polls)));
        assert_eq!(count(&probe.counts.nested_calls), 1);
        assert_eq!(count(&probe.counts.event_drops), 1);
        assert_eq!(signals.completed().len(), 1);

        registration.close();
        super::super::support::run(&runtime, driver.as_mut()).unwrap();
        drain(&runtime, guard);
        assert_eq!(active_endpoint(&endpoint), 0);
        assert_eq!(bytes.used(), 0);
    }
}

fn check_body_close_race(close_point: ClosePoint) {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(10), SourceStep::Pending]);
        let (mut live, signals) = attach(&runtime, Arc::clone(&probe), profile);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        drop(initial);
        let LiveSupervision {
            endpoint,
            registration,
            mut driver,
            body,
            guard,
        } = live;
        let body = Arc::new(Mutex::new(Some(body)));
        let hook_ran = Arc::new(AtomicBool::new(false));
        let close = {
            let body = Arc::clone(&body);
            let hook_ran = Arc::clone(&hook_ran);
            move || {
                hook_ran.store(true, Ordering::SeqCst);
                drop(body.lock().unwrap().take().expect("one live body"));
            }
        };
        match close_point {
            ClosePoint::BeforeOffer => driver.as_ref().get_ref().before_offer_for_test(close),
            ClosePoint::AfterOfferBeforeDemand => {
                driver.as_ref().get_ref().before_demand_for_test(close)
            }
        }

        drive_until(&runtime, &mut driver, || {
            count(&probe.counts.source_drops) == 1
        });
        assert!(hook_ran.load(Ordering::SeqCst));
        assert!(body.lock().unwrap().is_none());
        assert_eq!(count(&probe.counts.source_polls), 1);
        assert_eq!(count(&probe.counts.nested_calls), 1);
        assert_eq!(count(&probe.counts.event_drops), 1);
        assert_eq!(signals.completed().len(), 1);

        registration.close();
        super::super::support::run(&runtime, driver.as_mut()).unwrap();
        drain(&runtime, guard);
        assert_eq!(active_endpoint(&endpoint), 0);
        assert_eq!(bytes.used(), 0);
    }
}
