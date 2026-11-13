//! Root timer fixtures use an absolute monotonic factory, not duration flags.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Wake, Waker};
use std::time::Duration;

use super::super::state::{SourceStep, State, count};
use super::support::{Fixture, SUBSCRIPTION, kind, messages_for};
use crate::SubscriptionLimits;

#[test]
fn heartbeat_does_not_start_context_or_poll_an_idle_source_again() {
    let probe = State::new(vec![SourceStep::Pending]);
    let mut fixture = Fixture::new(Arc::clone(&probe));
    fixture.socket.subscribe("idle", SUBSCRIPTION);
    fixture.until_source_pending(&probe);
    assert_eq!(count(&probe.counts.factory_calls), 1);
    fixture.advance(Duration::from_secs(15));
    assert!(fixture.poll_once().is_pending());
    assert_eq!(
        fixture.socket.messages(),
        [r#"{"type":"connection_ack"}"#, r#"{"type":"ping"}"#]
    );
    assert_eq!(count(&probe.counts.source_polls), 1);
    assert_eq!(count(&probe.counts.factory_calls), 1);
    fixture.finish();
}

#[test]
fn queued_expiry_is_enforced_without_another_input_or_source_event() {
    let limits = SubscriptionLimits {
        max_queue_age: Duration::from_secs(1),
        ..SubscriptionLimits::default()
    };
    let mut fixture = Fixture::with_limits(State::new(Vec::new()), limits);
    fixture.socket.0.lock().unwrap().ready = false;
    fixture.socket.subscribe("expired", "query { ping }");
    fixture.until(|driver| {
        driver
            .operation_state_for_test("expired")
            .is_some_and(|(_, _, queued)| queued)
    });
    fixture.advance(Duration::from_secs(1));
    assert!(fixture.poll_once().is_pending());
    fixture.socket.0.lock().unwrap().ready = true;
    let handle = fixture.socket.clone();
    fixture.until(|_| !messages_for(&handle, "expired").is_empty());
    assert_eq!(kind(&messages_for(&fixture.socket, "expired")[0]), "error");
    fixture.finish();
}

#[derive(Default)]
struct WakeCount(AtomicUsize);

impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn an_idle_operation_does_not_self_wake_the_connection_without_new_work() {
    let probe = State::new(vec![SourceStep::Pending]);
    let mut fixture = Fixture::new(Arc::clone(&probe));
    fixture.socket.subscribe("idle", SUBSCRIPTION);
    fixture.until_source_pending(&probe);
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(Arc::clone(&wakes));
    {
        let _entered = fixture.runtime.enter();
        assert!(
            Pin::new(fixture.driver.as_mut().unwrap())
                .poll(&mut Context::from_waker(&waker))
                .is_pending()
        );
    }
    assert_eq!(
        wakes.0.load(Ordering::SeqCst),
        0,
        "only actual new work or an armed timer may wake an idle connection"
    );
    fixture.finish();
}

#[test]
fn physical_send_deadline_wins_simultaneous_progress_without_new_input() {
    let probe = State::new(vec![SourceStep::Event(7), SourceStep::Pending]);
    let limits = SubscriptionLimits {
        send_no_progress_timeout: Duration::from_secs(1),
        ..SubscriptionLimits::default()
    };
    let mut fixture = Fixture::with_limits(Arc::clone(&probe), limits);
    fixture.socket.0.lock().unwrap().automatic_flush = false;
    fixture.socket.subscribe("stalled", SUBSCRIPTION);
    let handle = fixture.socket.clone();
    fixture.until(|_| messages_for(&handle, "stalled").len() == 1);
    let remaining = fixture.socket.current_len();
    fixture.advance(Duration::from_secs(1));
    fixture.socket.0.lock().unwrap().progress.push_back(
        crate::__streaming::WebSocketWriteProgress {
            accepted: remaining,
            flushed: true,
        },
    );
    fixture.until_closed();
    assert_eq!(fixture.socket.0.lock().unwrap().closes, [None]);
    assert_eq!(
        fixture.socket.0.lock().unwrap().progress.len(),
        1,
        "expired physical send must not consume ready progress"
    );
    assert_eq!(messages_for(&fixture.socket, "stalled").len(), 1);
    fixture.finish();
    assert_eq!(count(&probe.counts.source_drops), 1);
}
