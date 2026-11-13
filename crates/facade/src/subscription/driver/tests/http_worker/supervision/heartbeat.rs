//! Manual-clock heartbeat coalescing and retained-control failure proofs.

use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use chigiri_http::SubscriptionTransport;

use super::super::super::state::{
    EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count,
};
use super::super::support::{drain, profiles, run, runtime};
use super::aggregate_support::{attach_aggregate, next_accepted, poll_driver_once, set_time};
use super::support::{
    active_endpoint, assert_complete, assert_safe_error, drive_until, drive_until_sources_pending,
    poll_body_once,
};

const HEARTBEAT: Duration = Duration::from_secs(1);

#[test]
fn repeated_ticks_coalesce_while_the_initial_body_is_unpolled() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Pending]);
        let (mut live, _) =
            attach_aggregate(&runtime, Arc::clone(&probe), &[profile], HEARTBEAT, false);
        for now in [1_000_000_000, 2_000_000_000, 3_000_000_000] {
            set_time(&mut live, now);
            assert!(matches!(
                poll_driver_once(&runtime, &mut live.driver),
                Poll::Pending
            ));
        }
        drive_until_sources_pending(&runtime, &mut live.driver, &probe, 1);
        assert_eq!(count(&probe.counts.source_polls), 1);

        let initial = next_accepted(&runtime, &mut live.driver, &mut live.bodies[0]).unwrap();
        assert!(initial.as_ref().is_empty());
        drop(initial);
        let heartbeat = next_accepted(&runtime, &mut live.driver, &mut live.bodies[0]).unwrap();
        assert_heartbeat(profile, heartbeat.as_ref());
        drop(heartbeat);
        assert!(matches!(
            poll_body_once(&runtime, &mut live.bodies[0]),
            Poll::Pending
        ));

        live.registration.close();
        run(&runtime, live.driver.as_mut()).unwrap();
        drop(live.bodies);
        drop(live.registration);
        drop(live.driver);
        drain(&runtime, live.guard);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(active_endpoint(&live.endpoint), 0);
    }
}

#[test]
fn held_heartbeat_delays_wire_terminal_but_not_worker_timeout_cleanup() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::modes(
            vec![SourceStep::Event(7)],
            SetupMode::Ready,
            EventMode::Pending,
            FactoryMode::Ready,
            FactoryMode::Ready,
            HookMode::Ready,
        );
        let (mut live, signals) =
            attach_aggregate(&runtime, Arc::clone(&probe), &[profile], HEARTBEAT, true);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        set_time(&mut live, 1_000_000_000);
        assert!(matches!(
            poll_driver_once(&runtime, &mut live.driver),
            Poll::Pending
        ));
        drop(next_accepted(
            &runtime,
            &mut live.driver,
            &mut live.bodies[0],
        ));
        let heartbeat = next_accepted(&runtime, &mut live.driver, &mut live.bodies[0]).unwrap();
        assert_heartbeat(profile, heartbeat.as_ref());
        let charged = heartbeat.as_ref().len();

        drive_until(&runtime, &mut live.driver, || {
            count(&probe.counts.event_future_drops) == 1
        });
        assert_eq!(count(&probe.counts.source_polls), 1);
        assert!(bytes.used() >= charged);
        assert!(matches!(
            poll_body_once(&runtime, &mut live.bodies[0]),
            Poll::Pending
        ));

        drop(heartbeat);
        let error = next_accepted(&runtime, &mut live.driver, &mut live.bodies[0]).unwrap();
        assert_safe_error(profile, error.as_ref(), "REQUEST_TIMEOUT");
        drop(error);
        let complete = next_accepted(&runtime, &mut live.driver, &mut live.bodies[0]).unwrap();
        assert_complete(profile, complete.as_ref());
        drop(complete);
        assert!(
            super::aggregate_support::next_frame(&runtime, &mut live.driver, &mut live.bodies[0],)
                .unwrap()
                .is_none()
        );
        assert_eq!(signals.completed().len(), 1);

        live.registration.close();
        run(&runtime, live.driver.as_mut()).unwrap();
        drop(live.bodies);
        drop(live.registration);
        drop(live.driver);
        drain(&runtime, live.guard);
        assert_eq!(active_endpoint(&live.endpoint), 0);
        assert_eq!(bytes.used(), 0);
    }
}

fn assert_heartbeat(profile: SubscriptionTransport, frame: &[u8]) {
    match profile {
        SubscriptionTransport::ServerSentEvents => assert_eq!(frame, b":\n\n"),
        SubscriptionTransport::Multipart => {
            let text = std::str::from_utf8(frame).unwrap();
            assert!(text.contains("Content-Type: application/json"));
            assert!(text.contains("{}"));
        }
        SubscriptionTransport::WebSocket => unreachable!(),
    }
}
