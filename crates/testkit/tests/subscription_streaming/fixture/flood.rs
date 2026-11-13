//! A finite but large source proves pressure cancellation before natural EOF.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::Stream;

use super::Tick;

const SOURCE_EVENTS: usize = 4096;

#[derive(Default)]
struct State {
    sources: AtomicUsize,
    produced: AtomicUsize,
    natural_end: AtomicBool,
    drops: AtomicUsize,
}

pub(crate) struct FloodProbe {
    started: Receiver<()>,
    dropped: Receiver<()>,
    state: Arc<State>,
}

struct Notifications {
    started: SyncSender<()>,
    dropped: SyncSender<()>,
}

pub(super) struct FloodFactory {
    notifications: Notifications,
    state: Arc<State>,
}

pub(super) fn source() -> (FloodFactory, FloodProbe) {
    let (started_sender, started) = mpsc::sync_channel(1);
    let (dropped_sender, dropped) = mpsc::sync_channel(1);
    let state = Arc::new(State::default());
    (
        FloodFactory {
            notifications: Notifications {
                started: started_sender,
                dropped: dropped_sender,
            },
            state: Arc::clone(&state),
        },
        FloodProbe {
            started,
            dropped,
            state,
        },
    )
}

impl FloodFactory {
    pub(super) fn source(&self) -> Flood {
        self.state.sources.fetch_add(1, Ordering::SeqCst);
        self.notifications
            .started
            .try_send(())
            .expect("one source initialization");
        Flood {
            produced: 0,
            dropped: self.notifications.dropped.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

impl FloodProbe {
    pub(crate) fn wait_until_pressure_cancels(&self) {
        self.started
            .recv_timeout(Duration::from_secs(5))
            .expect("source started");
        self.dropped
            .recv_timeout(Duration::from_secs(5))
            .expect("bounded slow-consumer policy cancels without a client read or disconnect");
        assert_eq!(self.state.sources.load(Ordering::SeqCst), 1);
        assert_eq!(self.state.drops.load(Ordering::SeqCst), 1);
        let produced = self.state.produced.load(Ordering::SeqCst);
        assert!(
            (2..SOURCE_EVENTS).contains(&produced),
            "pressure cut off at {produced}"
        );
        assert!(!self.state.natural_end.load(Ordering::SeqCst));
        eprintln!("live unread peer cancelled source after {produced}/{SOURCE_EVENTS} events");
    }
}

pub(super) struct Flood {
    produced: usize,
    dropped: SyncSender<()>,
    state: Arc<State>,
}

impl Stream for Flood {
    type Item = Result<Tick, chigiri::FieldError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.produced == SOURCE_EVENTS {
            self.state.natural_end.store(true, Ordering::SeqCst);
            return Poll::Ready(None);
        }
        self.produced += 1;
        self.state.produced.store(self.produced, Ordering::SeqCst);
        Poll::Ready(Some(Ok(Tick {
            value: self.produced as i32,
        })))
    }
}

impl Drop for Flood {
    fn drop(&mut self) {
        self.state.drops.fetch_add(1, Ordering::SeqCst);
        let _ = self.dropped.try_send(());
    }
}
