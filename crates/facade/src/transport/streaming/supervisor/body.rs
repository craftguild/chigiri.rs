//! Capacity-one body handoff, distinct acceptance and physical Drop notification.

use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use crate::subscription::bridge_resources::HttpSubscriptionPhysicalLease;
use crate::subscription::control::SubscriptionControlFrame;
use crate::subscription::outbox::SubscriptionSendFrame;
use crate::transport::streaming::SubscriptionInitialResponse;

use super::HttpStreamingError;
use super::clock::StreamClock;

mod accept;
mod fence;
#[cfg(test)]
mod test_faults;
#[cfg(test)]
mod tests;

pub(super) enum WireBytes {
    Initial(Arc<SubscriptionInitialResponse>),
    Event(SubscriptionSendFrame),
    Control(SubscriptionControlFrame),
}

impl AsRef<[u8]> for WireBytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Initial(value) => value.as_ref().as_ref(),
            Self::Event(value) => value.as_ref(),
            Self::Control(value) => value.as_ref(),
        }
    }
}

pub(super) struct BodyPort {
    state: Mutex<BodyState>,
    control: std::sync::OnceLock<Weak<crate::managed::Control>>,
    clock: Arc<StreamClock>,
    // Serializes accepted timestamps with pure common-policy transitions, not
    // source polling, application callbacks, or mailbox payload destruction.
    pub(super) timeline: Mutex<()>,
    #[cfg(test)]
    faults: Mutex<test_faults::Hooks>,
}

#[derive(Default)]
struct BodyState {
    offered: Option<HttpStreamingFrame>,
    generation: u64,
    live: bool,
    accepted: Option<(usize, u64)>,
    released: bool,
    body_closed: bool,
    eof: bool,
    error: Option<HttpStreamingError>,
    driver: Option<Waker>,
    body: Option<Waker>,
}

pub(super) struct BodyNotifications {
    pub(super) accepted: Option<(usize, u64)>,
    pub(super) released: bool,
    pub(super) closed: bool,
    pub(super) live: bool,
}

impl BodyPort {
    pub(super) fn new(clock: Arc<StreamClock>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(BodyState::default()),
            control: std::sync::OnceLock::new(),
            clock,
            timeline: Mutex::new(()),
            #[cfg(test)]
            faults: Mutex::new(test_faults::Hooks::default()),
        })
    }

    pub(super) fn offer(
        self: &Arc<Self>,
        bytes: WireBytes,
        physical: HttpSubscriptionPhysicalLease,
    ) -> Result<(), HttpStreamingError> {
        #[cfg(test)]
        self.run_before_offer_for_test();
        let mut state = self
            .state
            .lock()
            .map_err(|_| HttpStreamingError::Invariant)?;
        if state.body_closed {
            return Err(HttpStreamingError::Closed);
        }
        if state.live || state.eof || state.error.is_some() {
            return Err(HttpStreamingError::Invariant);
        }
        let generation = state
            .generation
            .checked_add(1)
            .ok_or(HttpStreamingError::Invariant)?;
        state.generation = generation;
        state.live = true;
        state.offered = Some(HttpStreamingFrame {
            bytes: Some(bytes),
            physical: Some(physical),
            port: Arc::downgrade(self),
            generation,
        });
        let wake = state.body.take();
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }

    pub(super) fn notifications(
        &self,
        context: &mut Context<'_>,
    ) -> Result<BodyNotifications, HttpStreamingError> {
        let mut replacement = Some(context.waker().clone());
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                drop(poisoned);
                drop(replacement);
                return Err(HttpStreamingError::Invariant);
            }
        };
        let previous = if state.body_closed {
            state.driver.take()
        } else {
            state
                .driver
                .replace(replacement.take().expect("one precloned driver waker"))
        };
        let notifications = BodyNotifications {
            accepted: state.accepted.take(),
            released: std::mem::take(&mut state.released),
            closed: state.body_closed,
            live: state.live,
        };
        drop(state);
        drop(previous);
        drop(replacement);
        Ok(notifications)
    }

    pub(super) fn finish(&self, error: Option<HttpStreamingError>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.eof {
            state.error = error;
        }
        state.eof = true;
        let offered = state.offered.take();
        let wake = state.body.take();
        drop(state);
        drop(offered);
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    fn close_body(&self) {
        #[cfg(test)]
        self.run_before_close_for_test();
        // A policy turn logically commits its next demand under this same
        // timeline. Closing before that turn prevents demand; closing after
        // it wakes supervision for immediate cancellation without emission.
        let timeline = self
            .timeline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.body_closed = true;
        let offered = state.offered.take();
        let wake = state.driver.take();
        drop(state);
        drop(timeline);
        drop(offered);
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    pub(super) fn is_closed(&self) -> Result<bool, HttpStreamingError> {
        self.state
            .lock()
            .map(|state| state.body_closed)
            .map_err(|_| HttpStreamingError::Invariant)
    }

    fn poll_body(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<HttpStreamingFrame, HttpStreamingError>>> {
        let mut replacement = Some(context.waker().clone());
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                drop(poisoned);
                drop(replacement);
                return Poll::Ready(Some(Err(HttpStreamingError::Invariant)));
            }
        };
        let outcome = if let Some(error) = state.error.take() {
            Poll::Ready(Some(Err(error)))
        } else if let Some(frame) = state.offered.take() {
            Poll::Ready(Some(Ok(frame)))
        } else if state.eof {
            Poll::Ready(None)
        } else {
            let previous = state
                .body
                .replace(replacement.take().expect("one precloned body waker"));
            drop(state);
            drop(previous);
            return Poll::Pending;
        };
        let previous = state.body.take();
        drop(state);
        drop(previous);
        drop(replacement);
        outcome
    }
}

/// A pull-only response body; it never polls source workers or policy timers.
pub struct HttpStreamingBody {
    pub(super) port: Arc<BodyPort>,
    pub(super) _initial: Arc<SubscriptionInitialResponse>,
    pub(super) _physical: HttpSubscriptionPhysicalLease,
}

impl HttpStreamingBody {
    /// Takes at most one complete entry without implicitly accepting its bytes.
    pub fn poll_next(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<HttpStreamingFrame, HttpStreamingError>>> {
        self.port.poll_body(context)
    }
}

impl Drop for HttpStreamingBody {
    fn drop(&mut self) {
        self.port.close_body();
    }
}

/// One unaccepted, immutable whole entry offered to an official body adapter.
pub struct HttpStreamingFrame {
    bytes: Option<WireBytes>,
    physical: Option<HttpSubscriptionPhysicalLease>,
    port: Weak<BodyPort>,
    generation: u64,
}

/// Accepted bytes whose original charge and active permit survive every clone.
///
/// This value is move-only. An ecosystem zero-copy owner may clone its own
/// references; the entire original owner remains until the last such Drop.
pub struct AcceptedHttpStreamingFrame {
    frame: HttpStreamingFrame,
}

impl AsRef<[u8]> for AcceptedHttpStreamingFrame {
    fn as_ref(&self) -> &[u8] {
        self.frame
            .bytes
            .as_ref()
            .expect("live physical frame")
            .as_ref()
    }
}
