//! Logical terminal observation is independent of physical HTTP frame cleanup.

use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::task::Poll;

use chigiri_runtime::__private::SubscriptionTermination;

use super::super::super::observation::install;
use super::super::super::state::{EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State};
use super::super::support::{drain, profiles, run, runtime};
use super::support::{
    assert_complete, attach, attach_with_real_deadlines, close, drive_until, next_frame,
    next_unaccepted_frame, text,
};

#[test]
fn http_observed_eof_freezes_drops_before_source_drop_and_prior_frame_drain() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new((1..=10).map(SourceStep::Event).collect());
        let host = install(&probe);
        let (mut live, _) = attach(&runtime, probe.clone(), profile);
        drive_until(&runtime, &mut live.driver, || {
            !host.terminals.lock().unwrap().is_empty()
        });
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [SubscriptionTermination::SourceComplete]
        );
        assert_eq!(*host.totals.lock().unwrap(), [8]);
        assert_eq!(*host.drops.lock().unwrap(), (1..=8).collect::<Vec<_>>());
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        // The initial physical frame remains unpolled when the decision wins.
        drop(next_frame(&runtime, &mut live).unwrap());
        for value in [9, 10] {
            let frame = next_frame(&runtime, &mut live).unwrap();
            assert!(text(&frame).contains(&format!("\"value\":{value}")));
            assert!(text(&frame).contains("\"droppedTotal\":8"));
            drop(frame);
        }
        let complete = next_frame(&runtime, &mut live).unwrap();
        assert_complete(profile, complete.as_ref());
        drop(complete);
        assert!(next_frame(&runtime, &mut live).is_none());
        close(&runtime, live);
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [SubscriptionTermination::SourceComplete]
        );
        assert_eq!(*host.totals.lock().unwrap(), [8]);
    }
}

#[test]
fn http_explicit_shutdown_and_peer_close_are_distinct_before_physical_cleanup() {
    let runtime = runtime();
    for profile in profiles() {
        for shutdown in [false, true] {
            let probe = State::new(vec![SourceStep::Pending]);
            let host = install(&probe);
            let (mut live, _) = attach(&runtime, Arc::clone(&probe), profile);
            let mut body = Some(live.body);
            if shutdown {
                live.registration.close();
            } else {
                body.take();
            }
            run(
                &runtime,
                poll_fn(|context| {
                    let polled = live.driver.as_mut().poll(context);
                    if !host.terminals.lock().unwrap().is_empty() {
                        Poll::Ready(())
                    } else {
                        assert!(
                            polled.is_pending(),
                            "decision must precede driver completion"
                        );
                        Poll::Pending
                    }
                }),
            );
            let decision = if shutdown {
                SubscriptionTermination::ServerShutdown
            } else {
                SubscriptionTermination::TransportClosed
            };
            assert_eq!(*host.terminals.lock().unwrap(), [decision]);
            assert_eq!(*host.totals.lock().unwrap(), [0]);
            assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
            live.registration.close();
            run(&runtime, live.driver.as_mut()).unwrap();
            drop(body);
            drain(&runtime, live.guard);
            assert_eq!(*host.terminals.lock().unwrap(), [decision]);
        }
    }
}

#[test]
fn http_source_failure_keeps_typed_execution_cause_through_shutdown() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Error]);
        let host = install(&probe);
        let (mut live, _) = attach(&runtime, probe.clone(), profile);
        drive_until(&runtime, &mut live.driver, || {
            !host.terminals.lock().unwrap().is_empty()
        });
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [SubscriptionTermination::ExecutionFailed]
        );
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        close(&runtime, live);
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [SubscriptionTermination::ExecutionFailed]
        );
        assert_eq!(*host.totals.lock().unwrap(), [0]);
    }
}

#[test]
fn http_deadline_delivery_and_typed_invariant_choose_distinct_immutable_causes() {
    let runtime = runtime();
    for profile in profiles() {
        for (mode, expected) in [
            (EventMode::Pending, SubscriptionTermination::ExecutionFailed),
            (
                EventMode::FrameworkInvariant,
                SubscriptionTermination::Internal,
            ),
            (EventMode::Object, SubscriptionTermination::DeliveryFailed),
        ] {
            let probe = State::modes(
                vec![SourceStep::Event(1), SourceStep::Pending],
                SetupMode::Ready,
                mode,
                FactoryMode::Ready,
                FactoryMode::Ready,
                HookMode::Ready,
            );
            let host = install(&probe);
            let (mut live, _) = if mode == EventMode::Pending {
                attach_with_real_deadlines(&runtime, probe.clone(), profile)
            } else {
                attach(&runtime, probe.clone(), profile)
            };
            let held = if mode == EventMode::Object {
                drop(next_frame(&runtime, &mut live).unwrap());
                Some(next_unaccepted_frame(&runtime, &mut live).unwrap().unwrap())
            } else {
                None
            };
            drive_until(&runtime, &mut live.driver, || {
                !host.terminals.lock().unwrap().is_empty()
            });
            assert_eq!(*host.terminals.lock().unwrap(), [expected]);
            assert_eq!(*host.totals.lock().unwrap(), [0]);
            assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
            drop(held);
            close(&runtime, live);
            assert_eq!(*host.terminals.lock().unwrap(), [expected]);
        }
    }
}

#[test]
fn http_logical_terminal_is_available_while_the_real_source_destructor_is_blocked() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(Vec::new());
        let gate = probe.block_source_drop();
        let host = install(&probe);
        let (mut live, _) = attach(&runtime, probe.clone(), profile);
        drive_until(&runtime, &mut live.driver, || {
            !host.terminals.lock().unwrap().is_empty()
        });
        gate.wait_entered();
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [SubscriptionTermination::SourceComplete]
        );
        assert_eq!(*host.source_drops_at_finish.lock().unwrap(), [0]);
        // Physical cleanup is still blocked, but later shutdown cannot replace
        // the already emitted decision or cause a second terminal attempt.
        live.registration.close();
        let _entered = runtime.enter();
        assert!(super::super::super::harness::poll_once(live.driver.as_mut()).is_pending());
        assert_eq!(
            *host.terminals.lock().unwrap(),
            [SubscriptionTermination::SourceComplete]
        );
        gate.release();
        close(&runtime, live);
    }
}
