use std::sync::Arc;

use super::super::super::state::{
    EventMode, FactoryMode, HookMode, SetupMode, SourceStep, State, count,
};
use super::super::support::{profiles, runtime};
use super::support::{
    active, assert_complete, assert_safe_error, attach, attach_with_real_deadlines, close,
    drive_until, next_frame, text,
};

#[test]
fn source_error_waits_for_the_last_event_owner_then_emits_safe_terminal_frames() {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![SourceStep::Event(10), SourceStep::Error]);
        let (mut live, signals) = attach(&runtime, Arc::clone(&probe), profile);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        let initial = next_frame(&runtime, &mut live).unwrap();
        drop(initial);
        let event = next_frame(&runtime, &mut live).expect("first ordinary event");
        assert!(text(&event).contains("\"value\":10"));
        drive_until(&runtime, &mut live.driver, || {
            count(&probe.counts.source_maps) == 1
        });
        assert_eq!(active(&live), 1);
        assert!(bytes.used() >= event.as_ref().len());

        drop(event);
        let error = next_frame(&runtime, &mut live).expect("masked source error");
        assert_safe_error(profile, error.as_ref(), "INTERNAL_SERVER_ERROR");
        drop(error);
        let complete = next_frame(&runtime, &mut live).expect("terminal completion");
        assert_complete(profile, complete.as_ref());
        drop(complete);
        assert!(next_frame(&runtime, &mut live).is_none());
        assert_eq!(count(&probe.counts.source_maps), 1);
        assert_eq!(signals.completed().len(), 1);

        close(&runtime, live);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(bytes.used(), 0);
    }
}

#[test]
fn event_timeout_waits_for_the_last_initial_owner_then_emits_safe_terminal_frames() {
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
        let (mut live, signals) = attach_with_real_deadlines(&runtime, Arc::clone(&probe), profile);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        drive_until(&runtime, &mut live.driver, || {
            count(&probe.counts.event_future_drops) == 1
        });
        assert_eq!(active(&live), 1);
        assert!(bytes.used() > 0);

        drop(initial);
        let error = next_frame(&runtime, &mut live).expect("safe timeout error");
        assert_safe_error(profile, error.as_ref(), "REQUEST_TIMEOUT");
        drop(error);
        let complete = next_frame(&runtime, &mut live).expect("timeout completion");
        assert_complete(profile, complete.as_ref());
        drop(complete);
        assert!(next_frame(&runtime, &mut live).is_none());
        assert_eq!(signals.completed().len(), 1);

        close(&runtime, live);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(count(&probe.counts.event_future_drops), 1);
        assert_eq!(bytes.used(), 0);
    }
}
