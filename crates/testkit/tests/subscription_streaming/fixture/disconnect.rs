use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::Stream;

use super::Tick;

pub(crate) struct DisconnectProbe {
    pending: Receiver<()>,
    dropped: Receiver<()>,
    state: Arc<ProbeState>,
}

#[derive(Clone)]
pub(super) struct PendingTicksFactory {
    pending: SyncSender<()>,
    dropped: SyncSender<()>,
    state: Arc<ProbeState>,
}

struct ProbeState {
    sources: AtomicUsize,
    pending_polls: AtomicUsize,
    drops: AtomicUsize,
}

pub(super) fn pending_ticks() -> (PendingTicksFactory, DisconnectProbe) {
    let (pending_sender, pending) = mpsc::sync_channel(1);
    let (dropped_sender, dropped) = mpsc::sync_channel(1);
    let state = Arc::new(ProbeState {
        sources: AtomicUsize::new(0),
        pending_polls: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
    });
    (
        PendingTicksFactory {
            pending: pending_sender,
            dropped: dropped_sender,
            state: Arc::clone(&state),
        },
        DisconnectProbe {
            pending,
            dropped,
            state,
        },
    )
}

impl PendingTicksFactory {
    pub(super) fn source(&self) -> PendingTicks {
        self.state.sources.fetch_add(1, Ordering::SeqCst);
        PendingTicks {
            next_value: 1,
            pending_reported: false,
            pending: self.pending.clone(),
            dropped: self.dropped.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

impl DisconnectProbe {
    pub(crate) fn wait_until_pending(&self) {
        self.pending
            .recv_timeout(Duration::from_secs(5))
            .expect("live subscription source must reach Pending");
        assert_eq!(self.state.sources.load(Ordering::SeqCst), 1);
        assert!(self.state.pending_polls.load(Ordering::SeqCst) >= 1);
        assert_eq!(self.state.drops.load(Ordering::SeqCst), 0);
    }

    pub(crate) fn wait_until_cancelled(&self) {
        self.dropped
            .recv_timeout(Duration::from_secs(5))
            .expect("peer disconnect must cancel the live subscription source");
    }

    pub(crate) fn assert_cancelled(self) {
        assert_eq!(self.state.sources.load(Ordering::SeqCst), 1);
        assert!(self.state.pending_polls.load(Ordering::SeqCst) >= 1);
        assert_eq!(self.state.drops.load(Ordering::SeqCst), 1);
    }
}

pub(super) struct PendingTicks {
    next_value: i32,
    pending_reported: bool,
    pending: SyncSender<()>,
    dropped: SyncSender<()>,
    state: Arc<ProbeState>,
}

impl Stream for PendingTicks {
    type Item = Result<Tick, chigiri::FieldError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.next_value <= 2 {
            let value = self.next_value;
            self.next_value += 1;
            return Poll::Ready(Some(Ok(Tick { value })));
        }
        self.state.pending_polls.fetch_add(1, Ordering::SeqCst);
        if !self.pending_reported {
            let _ = self.pending.try_send(());
            self.pending_reported = true;
        }
        Poll::Pending
    }
}

impl Drop for PendingTicks {
    fn drop(&mut self) {
        self.state.drops.fetch_add(1, Ordering::SeqCst);
        let _ = self.dropped.try_send(());
    }
}
