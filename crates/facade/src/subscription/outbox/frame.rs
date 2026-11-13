//! Immutable send ownership and payload-free progress authority.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chigiri_http::FinalizedSubscriptionEvent;

use super::super::delivery::{InFlightProgress, InFlightSend, MonotonicInstant};
use super::SubscriptionOutboxSignal;

#[derive(Debug, Default)]
pub(super) struct SharedTerminal {
    signal: Mutex<Option<SubscriptionOutboxSignal>>,
}

impl SharedTerminal {
    pub(super) fn get(&self) -> Result<Option<SubscriptionOutboxSignal>, ()> {
        self.signal.lock().map(|state| *state).map_err(|_| ())
    }

    pub(super) fn set(&self, signal: SubscriptionOutboxSignal) -> SubscriptionOutboxSignal {
        let Ok(mut state) = self.signal.lock() else {
            return SubscriptionOutboxSignal::Invariant;
        };
        if state.is_none() {
            *state = Some(signal);
        }
        state.unwrap_or(SubscriptionOutboxSignal::Invariant)
    }
}

#[derive(Debug, Default)]
pub(super) struct SharedClock {
    last_now_ns: AtomicU64,
}

impl SharedClock {
    pub(super) fn observe(&self, now_ns: u64) -> bool {
        let mut previous = self.last_now_ns.load(Ordering::Acquire);
        loop {
            if now_ns < previous {
                return false;
            }
            match self.last_now_ns.compare_exchange_weak(
                previous,
                now_ns,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => previous = observed,
            }
        }
    }
}

/// A move-only immutable event frame for zero-copy bridge ownership.
///
/// The exact endpoint byte charge remains attached until this value and every
/// zero-copy owner containing it are dropped.
pub struct SubscriptionSendFrame {
    send: InFlightSend<FinalizedSubscriptionEvent>,
}

impl SubscriptionSendFrame {
    pub(super) fn new(send: InFlightSend<FinalizedSubscriptionEvent>) -> Self {
        Self { send }
    }

    /// Returns the complete framed byte length.
    pub fn len(&self) -> usize {
        self.send.bytes()
    }

    /// Returns whether the framed event has no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrows immutable bytes for a body or socket write.
    pub fn as_bytes(&self) -> &[u8] {
        self.send.payload().as_bytes()
    }
}

impl AsRef<[u8]> for SubscriptionSendFrame {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for SubscriptionSendFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionSendFrame")
            .field("bytes", &self.len())
            .finish_non_exhaustive()
    }
}

/// A move-only capability for recording accepted bytes without owning payload.
pub struct SubscriptionSendProgress {
    pub(super) inner: InFlightProgress,
    clock: Arc<SharedClock>,
    terminal: Arc<SharedTerminal>,
}

impl SubscriptionSendProgress {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn retire(self, timeout: std::time::Duration) -> super::RetiredSendProgress {
        super::RetiredSendProgress::new(self.inner, self.clock, timeout)
    }

    pub(super) fn new(
        inner: InFlightProgress,
        clock: Arc<SharedClock>,
        terminal: Arc<SharedTerminal>,
    ) -> Self {
        Self {
            inner,
            clock,
            terminal,
        }
    }

    /// Returns the number of bytes accepted by the bridge.
    pub fn accepted_bytes(&self) -> Result<usize, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        self.inner
            .accepted_bytes()
            .map_err(|_| self.terminal.set(SubscriptionOutboxSignal::Accounting))
    }

    /// Records an accepted-byte count at the bridge monotonic timestamp.
    ///
    /// Zero bytes never reset the common send-progress timer. The returned
    /// Boolean is true only when all bytes have been accepted.
    pub fn accept(&mut self, bytes: usize, now_ns: u64) -> Result<bool, SubscriptionOutboxSignal> {
        self.check_terminal()?;
        if !self.clock.observe(now_ns) {
            return Err(self
                .terminal
                .set(SubscriptionOutboxSignal::NonMonotonicTime));
        }
        self.inner
            .accept(bytes, MonotonicInstant::from_nanos(now_ns))
            .map_err(|_| self.terminal.set(SubscriptionOutboxSignal::Accounting))
    }

    fn check_terminal(&self) -> Result<(), SubscriptionOutboxSignal> {
        match self.terminal.get() {
            Ok(None) => Ok(()),
            Ok(Some(signal)) => Err(signal),
            Err(()) => Err(SubscriptionOutboxSignal::Invariant),
        }
    }
}

impl fmt::Debug for SubscriptionSendProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionSendProgress")
            .field("accepted_bytes", &self.accepted_bytes().ok())
            .finish_non_exhaustive()
    }
}
