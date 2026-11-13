//! Exact-generation timer transitions while the sole outer poll is in flight.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

use chigiri_http::BoxFuture;

use super::state::DeadlineState;
use super::{DeadlineRole, FiredDeadline};
use crate::subscription::worker::SubscriptionWorkerRuntimeError;

mod http_evidence;
mod registered;
mod waker;

struct BlockingTimer {
    entered: SyncSender<()>,
    released: Receiver<()>,
    first_poll: bool,
    ready: bool,
    drops: Arc<AtomicUsize>,
}

impl Future for BlockingTimer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.first_poll {
            self.first_poll = false;
            self.entered.send(()).unwrap();
            self.released.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        if self.ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for BlockingTimer {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct PollGate {
    entered: Receiver<()>,
    release: SyncSender<()>,
    drops: Arc<AtomicUsize>,
}

struct ReleaseOnDrop(SyncSender<()>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn blocked_timer(ready: bool) -> (BoxFuture<'static, ()>, PollGate) {
    let (entered, entered_receiver) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let drops = Arc::new(AtomicUsize::new(0));
    (
        Box::pin(BlockingTimer {
            entered,
            released,
            first_poll: true,
            ready,
            drops: Arc::clone(&drops),
        }),
        PollGate {
            entered: entered_receiver,
            release,
            drops,
        },
    )
}

type Expiry = Poll<Result<FiredDeadline, SubscriptionWorkerRuntimeError>>;

#[derive(Default)]
struct PollWake(AtomicUsize);

impl Wake for PollWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn during_poll<T>(
    state: &Arc<DeadlineState>,
    gate: PollGate,
    action: impl FnOnce() -> T,
) -> (Expiry, T, usize) {
    std::thread::scope(|scope| {
        let notifications = Arc::new(PollWake::default());
        let poll_notifications = Arc::clone(&notifications);
        let poller = scope.spawn(move || {
            let waker = Waker::from(poll_notifications);
            state.poll_expired(&mut Context::from_waker(&waker))
        });
        let release = ReleaseOnDrop(gate.release);
        gate.entered.recv_timeout(Duration::from_secs(3)).unwrap();
        let result = action();
        drop(release);
        let expiry = poller.join().unwrap();
        (expiry, result, notifications.0.load(Ordering::SeqCst))
    })
}

fn poll(state: &DeadlineState) -> Expiry {
    state.poll_expired(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn proxy_can_bind_while_the_original_timer_is_being_polled_outside_the_lock() {
    let (timer, gate) = blocked_timer(false);
    let state = DeadlineState::with_overall(timer).unwrap();
    let (expiry, proxy, _) = during_poll(&state, gate, || state.proxy(DeadlineRole::Overall));
    assert_eq!(expiry, Poll::Pending);
    let mut proxy = proxy.expect("poll extraction does not change timer identity");
    assert_eq!(
        Pin::new(&mut proxy).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    proxy.clear().unwrap();
    assert_eq!(poll(&state), Poll::Pending);
}

#[test]
fn clearing_current_timer_during_poll_is_a_valid_commit_transition() {
    let (timer, gate) = blocked_timer(true);
    let drops = Arc::clone(&gate.drops);
    let state = DeadlineState::with_overall(timer).unwrap();
    let (expiry, cleared, _) =
        during_poll(&state, gate, || state.clear_current(DeadlineRole::Overall));
    cleared.expect("initial commit may clear an in-flight original timer");
    assert_eq!(expiry, Poll::Pending);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(poll(&state), Poll::Pending);
}

#[test]
fn immediate_phase_rearm_ignores_the_old_inflight_ready_timer() {
    let state = DeadlineState::with_overall(Box::pin(std::future::pending())).unwrap();
    let (timer, gate) = blocked_timer(true);
    let drops = Arc::clone(&gate.drops);
    let previous = state.arm(DeadlineRole::Phase, timer).unwrap();
    let ready = Arc::new(AtomicBool::new(false));
    let next_ready = Arc::clone(&ready);
    let (expiry, next, notifications) = during_poll(&state, gate, || {
        previous.clear().unwrap();
        state.arm(
            DeadlineRole::Phase,
            Box::pin(std::future::poll_fn(move |_| {
                if next_ready.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })),
        )
    });
    let mut next = next.expect("a cleared generation cannot block the next phase");
    assert_eq!(expiry, Poll::Pending);
    assert!(
        notifications > 0,
        "replacement must schedule the next outer turn"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(poll(&state), Poll::Pending);
    assert_eq!(
        Pin::new(&mut next).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
    ready.store(true, Ordering::SeqCst);
    let Poll::Ready(Ok(fired)) = poll(&state) else {
        panic!("the replacement timer must still be armed");
    };
    assert_eq!(fired.role, DeadlineRole::Phase);
    assert_eq!(fired.generation, 2);
    assert_eq!(
        Pin::new(&mut next).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
}

#[test]
fn a_proxy_issued_after_expiry_observes_the_same_original_generation() {
    let state = DeadlineState::with_overall(Box::pin(async {})).unwrap();
    let Poll::Ready(Ok(fired)) = poll(&state) else {
        panic!("the original deadline is ready");
    };
    assert_eq!(fired.role, DeadlineRole::Overall);
    let mut proxy = state
        .proxy(DeadlineRole::Overall)
        .expect("expiry is not a proxy issuance invariant failure");
    assert_eq!(
        Pin::new(&mut proxy).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
}

#[test]
fn relinquished_setup_proxy_preserves_the_original_timer_for_the_next_worker() {
    let ready = Arc::new(AtomicBool::new(false));
    let timer_ready = Arc::clone(&ready);
    let state = DeadlineState::with_setup_only(Box::pin(std::future::poll_fn(move |_| {
        if timer_ready.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })))
    .unwrap();
    let first = state.proxy(DeadlineRole::Setup).unwrap();
    first.relinquish();
    assert_eq!(poll(&state), Poll::Pending);
    let mut next = state
        .proxy(DeadlineRole::Setup)
        .expect("handoff must not abandon the externally owned setup timer");
    ready.store(true, Ordering::SeqCst);
    let Poll::Ready(Ok(fired)) = poll(&state) else {
        panic!("the original timer must fire");
    };
    assert_eq!(fired.role, DeadlineRole::Setup);
    assert_eq!(fired.generation, 1);
    assert_eq!(
        Pin::new(&mut next).poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(())
    );
}
