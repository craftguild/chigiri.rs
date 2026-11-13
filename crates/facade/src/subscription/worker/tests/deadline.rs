use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};

use super::super::{DeadlineRole, MonitoredDeadlineTable};

struct EventDeadlines {
    ready: AtomicBool,
    calls: AtomicUsize,
}

impl DeadlineFactory for EventDeadlines {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'_, ()> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Box::pin(std::future::poll_fn(move |_| {
            if self.ready.load(Ordering::Acquire) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}

fn event_deadlines() -> &'static EventDeadlines {
    Box::leak(Box::new(EventDeadlines {
        ready: AtomicBool::new(false),
        calls: AtomicUsize::new(0),
    }))
}

fn pending_timer() -> BoxFuture<'static, ()> {
    Box::pin(std::future::pending())
}

fn flag_timer(flag: Arc<AtomicBool>) -> BoxFuture<'static, ()> {
    Box::pin(std::future::poll_fn(move |_| {
        if flag.load(Ordering::Acquire) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }))
}

#[test]
fn event_timer_is_unarmed_while_idle_and_expires_from_the_outer_table() {
    let deadlines = event_deadlines();
    let (mut table, armer) =
        MonitoredDeadlineTable::with_setup(deadlines, pending_timer(), pending_timer()).unwrap();
    assert_eq!(deadlines.calls.load(Ordering::Acquire), 0);

    let mut event = armer
        .arm(DeadlineRole::Event, Duration::from_secs(3))
        .unwrap();
    assert_eq!(deadlines.calls.load(Ordering::Acquire), 1);
    let mut context = Context::from_waker(Waker::noop());
    assert_eq!(table.poll_expired(&mut context), Poll::Pending);
    assert_eq!(
        std::pin::Pin::new(&mut event).poll(&mut context),
        Poll::Pending
    );

    deadlines.ready.store(true, Ordering::Release);
    let fired = match table.poll_expired(&mut context) {
        Poll::Ready(Ok(fired)) => fired,
        other => panic!("event deadline did not fire: {other:?}"),
    };
    assert_eq!(fired.role, DeadlineRole::Event);
    assert_eq!(
        std::pin::Pin::new(&mut event).poll(&mut context),
        Poll::Ready(())
    );
}

#[test]
fn stale_generation_and_duplicate_arm_fail_closed() {
    let deadlines = event_deadlines();
    let (table, armer) =
        MonitoredDeadlineTable::with_setup(deadlines, pending_timer(), pending_timer()).unwrap();
    let event = armer
        .arm(DeadlineRole::Event, Duration::from_secs(1))
        .unwrap();
    assert!(
        armer
            .arm(DeadlineRole::Event, Duration::from_secs(1))
            .is_err()
    );
    drop(event);
    table.close();
    assert!(
        armer
            .arm(DeadlineRole::Phase, Duration::from_secs(1))
            .is_err()
    );
}

#[test]
fn setup_handoff_keeps_the_same_overall_timer_until_initial_commit() {
    let overall = Arc::new(AtomicBool::new(false));
    let setup = Arc::new(AtomicBool::new(false));
    let deadlines = event_deadlines();
    let (mut table, armer) = MonitoredDeadlineTable::with_setup(
        deadlines,
        flag_timer(Arc::clone(&overall)),
        flag_timer(Arc::clone(&setup)),
    )
    .unwrap();
    armer
        .setup_pair()
        .unwrap()
        .handoff_overall_and_clear_setup()
        .unwrap();
    let mut context = Context::from_waker(Waker::noop());

    setup.store(true, Ordering::Release);
    assert_eq!(table.poll_expired(&mut context), Poll::Pending);
    overall.store(true, Ordering::Release);
    let fired = match table.poll_expired(&mut context) {
        Poll::Ready(Ok(fired)) => fired,
        other => panic!("overall deadline did not remain armed: {other:?}"),
    };
    assert_eq!(fired.role, DeadlineRole::Overall);
}
