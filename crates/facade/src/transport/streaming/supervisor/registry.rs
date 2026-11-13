//! Fallibly preallocated registration slots, not an event queue or task pool.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;

use chigiri_http::ResponseHeader;

use super::HttpStreamingError;
use super::body::{BodyPort, HttpStreamingBody, WireBytes};
use super::clock::StreamClock;
use super::driver::HttpStreamingDriver;
use super::entry::Entry;
use crate::subscription::delivery::EndpointByteOwner;
use crate::transport::streaming::{ReadyHttpSubscription, SubscriptionInitialResponse};

pub(super) struct Registry {
    pub(super) state: Mutex<RegistryState>,
    shutdown_requested: AtomicBool,
    bytes: EndpointByteOwner,
    clock: Arc<StreamClock>,
}

pub(super) struct RegistryState {
    pub(super) open: bool,
    pub(super) slots: Vec<RegistrationSlot>,
    pub(super) driver: Option<Waker>,
}

pub(super) struct RegistrationSlot {
    pub(super) occupied: bool,
    pub(super) pending: Option<Entry>,
}

struct RegistrationLife {
    registry: Arc<Registry>,
}

impl Drop for RegistrationLife {
    fn drop(&mut self) {
        self.registry.close();
    }
}

/// Cloneable bounded registration authority for one explicit HTTP bridge.
#[derive(Clone)]
pub struct HttpStreamingRegistration {
    life: Arc<RegistrationLife>,
}

impl HttpStreamingRegistration {
    pub(crate) fn new(
        bytes: EndpointByteOwner,
        maximum: usize,
    ) -> Result<(Self, HttpStreamingDriver), HttpStreamingError> {
        if maximum == 0 {
            return Err(HttpStreamingError::Invariant);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(maximum)
            .map_err(|_| HttpStreamingError::Capacity)?;
        slots.extend((0..maximum).map(|_| RegistrationSlot {
            occupied: false,
            pending: None,
        }));
        let clock = Arc::new(StreamClock::new());
        let registry = Arc::new(Registry {
            shutdown_requested: AtomicBool::new(false),
            state: Mutex::new(RegistryState {
                open: true,
                slots,
                driver: None,
            }),
            bytes,
            clock: Arc::clone(&clock),
        });
        let driver = HttpStreamingDriver::new(Arc::clone(&registry), clock, maximum)?;
        Ok((
            Self {
                life: Arc::new(RegistrationLife { registry }),
            },
            driver,
        ))
    }

    /// Attaches one already committed source from the same endpoint graph.
    ///
    /// This does not acquire another subscription permit, start a task, wait
    /// for capacity, or copy its payload. Failure returns no response head.
    pub fn attach(
        &self,
        ready: ReadyHttpSubscription,
    ) -> Result<SupervisedHttpResponse, HttpStreamingError> {
        self.attach_inner(ready, None)
    }

    pub(crate) fn attach_managed(
        &self,
        ready: ReadyHttpSubscription,
        control: &crate::managed::Control,
    ) -> Result<SupervisedHttpResponse, HttpStreamingError> {
        self.attach_inner(ready, Some(control))
    }

    fn attach_inner(
        &self,
        ready: ReadyHttpSubscription,
        control: Option<&crate::managed::Control>,
    ) -> Result<SupervisedHttpResponse, HttpStreamingError> {
        let registry = &self.life.registry;
        let (initial, outbox, events, configuration) = ready.into_supervision();
        if !registry.bytes.same_owner(&configuration.bytes) {
            return Err(HttpStreamingError::Invariant);
        }
        let physical = initial
            .physical_lease()
            .ok_or(HttpStreamingError::Invariant)?;
        let initial = Arc::new(initial);
        let port = BodyPort::new(Arc::clone(&registry.clock));
        let mut entry = Some(Entry::new(
            Arc::clone(&port),
            physical.clone(),
            events,
            outbox,
            configuration,
            Arc::clone(&registry.clock),
        )?);
        port.offer(WireBytes::Initial(Arc::clone(&initial)), physical.clone())?;
        let gate = control
            .map(|control| control.registration_gate())
            .transpose()
            .map_err(|_| HttpStreamingError::Closed)?;
        let mut state = registry
            .state
            .lock()
            .map_err(|_| HttpStreamingError::Invariant)?;
        if !state.open {
            return Err(HttpStreamingError::Closed);
        }
        let slot = state
            .slots
            .iter_mut()
            .find(|slot| !slot.occupied)
            .ok_or(HttpStreamingError::Capacity)?;
        slot.occupied = true;
        slot.pending = entry.take();
        let wake = state.driver.take();
        drop(state);
        drop(gate);
        if let Some(wake) = wake {
            wake.wake();
        }
        let body = HttpStreamingBody {
            port,
            _initial: Arc::clone(&initial),
            _physical: physical,
        };
        Ok(SupervisedHttpResponse { initial, body })
    }

    /// Rejects new attachments and asks the sole driver to cancel and drain.
    pub fn close(&self) {
        self.life.registry.close();
    }
}

impl Registry {
    pub(super) fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }

    pub(super) fn close(&self) {
        // Application close is monotonic authority independent of a later
        // poisoned mailbox. Internal driver abandonment does not set it.
        self.shutdown_requested.store(true, Ordering::Release);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.open = false;
        let wake = state.driver.take();
        drop(state);
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

/// Committed immutable response head and its matching supervised body.
pub struct SupervisedHttpResponse {
    initial: Arc<SubscriptionInitialResponse>,
    body: HttpStreamingBody,
}

impl SupervisedHttpResponse {
    /// Returns the already committed status without permitting replacement.
    pub fn status(&self) -> u16 {
        self.initial.status()
    }
    /// Borrows the already committed headers for the server's response head.
    pub fn headers(&self) -> &[ResponseHeader] {
        self.initial.headers()
    }
    /// Moves the body without copying initial or subsequent frame bytes.
    pub fn into_body(self) -> HttpStreamingBody {
        self.body
    }
}
