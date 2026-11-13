use std::future::{Future, poll_fn};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::Poll;

use super::super::super::state::{SourceStep, State, count};
use super::super::support::{drain, profiles, run, runtime};
use super::support::{active_endpoint, attach, next_frame};

#[test]
fn event_payload_drops_after_the_timeline_guard_during_encoding_unwind() {
    check_encoding_unwind(SourceStep::Event(1), false);
}

#[test]
fn terminal_payload_drops_after_the_timeline_guard_during_encoding_unwind() {
    check_encoding_unwind(SourceStep::Error, true);
}

fn check_encoding_unwind(source: SourceStep, terminal: bool) {
    let runtime = runtime();
    for profile in profiles() {
        let probe = State::new(vec![source]);
        let (mut live, signals) = attach(&runtime, Arc::clone(&probe), profile);
        let endpoint = Arc::clone(&live.endpoint);
        let bytes = endpoint.subscriptions.resources.delivery().byte_owner();

        let initial = next_frame(&runtime, &mut live).expect("committed initial frame");
        drop(initial);
        let marker = live
            .driver
            .as_ref()
            .get_ref()
            .panic_on_encode_for_test(terminal);
        let panic = catch_unwind(AssertUnwindSafe(|| {
            run(
                &runtime,
                poll_fn(|context| match live.driver.as_mut().poll(context) {
                    Poll::Pending => Poll::<()>::Pending,
                    Poll::Ready(result) => {
                        panic!("driver ended before injected encoding panic: {result:?}")
                    }
                }),
            )
        }));
        assert!(panic.is_err());
        assert_eq!(marker.load(Ordering::SeqCst), 1);
        if terminal {
            assert_eq!(count(&probe.counts.source_maps), 1);
        } else {
            assert_eq!(count(&probe.counts.nested_calls), 1);
        }

        drop(live.driver);
        drop(live.body);
        drop(live.registration);
        drain(&runtime, live.guard);
        assert_eq!(count(&probe.counts.source_drops), 1);
        assert_eq!(signals.completed().len(), 1);
        assert_eq!(active_endpoint(&endpoint), 0);
        assert_eq!(bytes.used(), 0);
    }
}
