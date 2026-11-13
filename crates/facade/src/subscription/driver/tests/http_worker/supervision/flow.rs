use std::sync::Arc;

use chigiri_runtime::LifecycleResult;

use super::super::super::state::{SourceStep, State, count};
use super::super::support::{profiles, runtime};
use super::support::{active, assert_complete, attach, close, drive_until, next_frame, text};

#[test]
fn unpolled_initial_allows_drop_oldest_to_keep_the_last_two_events_in_order() {
    let runtime = runtime();
    for profile in profiles() {
        let source = (1..=10).map(SourceStep::Event).collect();
        let probe = State::new(source);
        let (mut live, signals) = attach(&runtime, Arc::clone(&probe), profile);
        let bytes = live
            .endpoint
            .subscriptions
            .resources
            .delivery()
            .byte_owner();

        drive_until(&runtime, &mut live.driver, || {
            count(&probe.counts.source_polls) == 11
        });
        assert_eq!(count(&probe.counts.nested_calls), 10);
        assert_eq!(count(&probe.counts.event_drops), 10);
        assert_eq!(active(&live), 1);
        assert!(bytes.used() > 0);

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        assert!(initial.as_ref().is_empty());
        drop(initial);

        for value in [9, 10] {
            let frame = next_frame(&runtime, &mut live).expect("surviving event frame");
            let text = text(&frame);
            assert!(
                text.contains(&format!("\"value\":{value}")),
                "unexpected survivor: {text}"
            );
            assert!(
                text.contains("\"droppedTotal\":8"),
                "missing exact eviction count: {text}"
            );
            drop(frame);
        }
        let complete = next_frame(&runtime, &mut live).expect("natural completion frame");
        assert_complete(profile, complete.as_ref());
        drop(complete);
        assert!(next_frame(&runtime, &mut live).is_none());
        assert_eq!(signals.completed(), [(LifecycleResult::Succeeded, None)]);

        close(&runtime, live);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(bytes.used(), 0);
    }
}
