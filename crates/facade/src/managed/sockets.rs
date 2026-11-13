//! Fixed connection slots for charged SDK WebSocket upgrade/session ownership.

mod entry;
mod integrity;
mod io;
mod tracked;

pub(crate) use integrity::SocketScope;

use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use super::{Control, ManagedServiceError, StopPhase};
use crate::__streaming::{ClaimedWebSocketUpgrade, WebSocketIo, WebSocketIoError};
use crate::{BoxFuture, DeadlineFactory, Endpoint};

pub(crate) trait SocketWork: Send {
    fn poll(
        &mut self,
        context: &mut Context<'_>,
        phase: StopPhase,
    ) -> Poll<Result<(), ManagedServiceError>>;
}

pub(crate) struct SocketRegistry {
    control: Arc<Control>,
    runtime: tokio::runtime::Handle,
    state: Mutex<State>,
}

struct State {
    open: bool,
    slots: Vec<Slot>,
    driver: Option<Waker>,
}

struct Slot {
    occupied: bool,
    pending: Option<Box<dyn SocketWork>>,
    retained: Option<Arc<SocketScope>>,
}

pub(crate) struct SocketDriver {
    registry: Arc<SocketRegistry>,
    entries: Vec<Option<Box<dyn SocketWork>>>,
    cursor: usize,
}

impl SocketRegistry {
    #[cfg(test)]
    pub(crate) fn retained_tickets_for_test(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .slots
            .iter()
            .filter(|slot| {
                slot.retained
                    .as_ref()
                    .is_some_and(|scope| scope.retains_ticket_for_test())
            })
            .count()
    }

    pub(crate) fn has_poisoned(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .slots
            .iter()
            .any(|slot| slot.retained.as_ref().is_some_and(|scope| scope.poisoned()))
    }

    pub(crate) fn new(
        control: Arc<Control>,
        runtime: tokio::runtime::Handle,
        capacity: usize,
    ) -> Result<(Arc<Self>, SocketDriver), ManagedServiceError> {
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(capacity)
            .map_err(|_| ManagedServiceError::Capacity)?;
        slots.resize_with(capacity, || Slot {
            occupied: false,
            pending: None,
            retained: None,
        });
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(capacity)
            .map_err(|_| ManagedServiceError::Capacity)?;
        entries.resize_with(capacity, || None);
        let registry = Arc::new(Self {
            control,
            runtime,
            state: Mutex::new(State {
                open: true,
                slots,
                driver: None,
            }),
        });
        let driver = SocketDriver {
            registry: Arc::clone(&registry),
            entries,
            cursor: 0,
        };
        Ok((registry, driver))
    }

    fn attach(
        &self,
        work: Box<dyn SocketWork>,
        scope: Arc<SocketScope>,
    ) -> Result<(), ManagedServiceError> {
        // Declare the application-adjacent owner first so every early-return
        // path destroys it only after both admission and mailbox guards drop.
        let mut proposed = Some(work);
        let gate = self.control.registration_gate()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| ManagedServiceError::Infrastructure)?;
        if !state.open {
            return Err(ManagedServiceError::Closed);
        }
        let slot = state
            .slots
            .iter_mut()
            .find(|slot| !slot.occupied)
            .ok_or(ManagedServiceError::Capacity)?;
        slot.occupied = true;
        slot.retained = Some(scope);
        slot.pending = proposed.take();
        let wake = state.driver.take();
        drop(state);
        drop(gate);
        if let Some(wake) = wake {
            wake.wake();
        }
        Ok(())
    }
}

impl SocketDriver {
    pub(super) fn registry(&self) -> Arc<SocketRegistry> {
        Arc::clone(&self.registry)
    }
    pub(crate) fn poll(
        &mut self,
        context: &mut Context<'_>,
        phase: StopPhase,
    ) -> Poll<Result<(), ManagedServiceError>> {
        let replacement = context.waker().clone();
        let mut state = self
            .registry
            .state
            .lock()
            .map_err(|_| ManagedServiceError::Infrastructure)?;
        let previous = state.driver.replace(replacement);
        if state
            .slots
            .iter()
            .enumerate()
            .any(|(index, slot)| slot.pending.is_some() && self.entries[index].is_some())
        {
            drop(state);
            drop(previous);
            return Poll::Ready(Err(ManagedServiceError::Infrastructure));
        }
        for (index, slot) in state.slots.iter_mut().enumerate() {
            if let Some(entry) = slot.pending.take() {
                self.entries[index] = Some(entry);
            }
        }
        if phase != StopPhase::Running {
            state.open = false;
        }
        drop(state);
        drop(previous);
        for offset in 0..self.entries.len() {
            let index = (self.cursor + offset) % self.entries.len();
            let Some(entry) = &mut self.entries[index] else {
                continue;
            };
            if let Poll::Ready(result) = entry.poll(context, phase) {
                drop(self.entries[index].take());
                let mut state = self
                    .registry
                    .state
                    .lock()
                    .map_err(|_| ManagedServiceError::Infrastructure)?;
                state.slots[index].occupied = false;
                drop(state);
                result?;
            }
        }
        self.cursor = (self.cursor + 1) % self.entries.len();
        if phase != StopPhase::Running && self.entries.iter().all(Option::is_none) {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl Drop for SocketDriver {
    fn drop(&mut self) {
        let (count, wake) = {
            let mut state = self
                .registry
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.open = false;
            (state.slots.len(), state.driver.take())
        };
        if let Some(wake) = wake
            && let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wake.wake()))
        {
            let _ = crate::subscription::worker::dispose_payload(payload);
            self.registry.control.fail();
        }
        for index in 0..count {
            let pending = {
                let mut state = self
                    .registry
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                state.slots[index].pending.take()
            };
            drop(pending);
        }
        // Session Drop transfers its concrete workers to the original charged
        // quarantine. Never destroy socket/application values under the lock.
        self.entries.clear();
    }
}

/// Opaque same-endpoint upgrade, including its original connection permit.
#[doc(hidden)]
pub struct ManagedWebSocketUpgrade {
    seed: ClaimedWebSocketUpgrade<'static>,
    registry: Arc<SocketRegistry>,
}

impl ManagedWebSocketUpgrade {
    pub(crate) fn new(
        seed: ClaimedWebSocketUpgrade<'static>,
        registry: Arc<SocketRegistry>,
    ) -> Self {
        Self { seed, registry }
    }

    /// Returns the validated key for the native library's accept calculation.
    pub fn key(&self) -> &str {
        self.seed.key()
    }
    /// Returns the one negotiated application subprotocol.
    pub fn protocol(&self) -> &'static str {
        self.seed.protocol()
    }

    pub(crate) fn attach<C: Send + Sync + 'static, S: WebSocketIo>(
        self,
        endpoint: &Endpoint<C>,
        socket: BoxFuture<'static, Result<S, WebSocketIoError>>,
        deadlines: &'static dyn DeadlineFactory,
    ) -> Result<(), ManagedServiceError> {
        if !Arc::ptr_eq(&self.registry.control, &endpoint.managed_control()) {
            return Err(ManagedServiceError::Infrastructure);
        }
        let physical = self
            .seed
            .native_physical_lease()
            .map_err(|_| ManagedServiceError::Infrastructure)?;
        let scope = SocketScope::new(Arc::downgrade(&self.registry.control), physical);
        let instrumentation = self.seed.instrumentation().map(|captured| {
            captured.handoff_scope(
                chigiri_runtime::instrumentation::InstrumentationKind::Provider {
                    phase: chigiri_runtime::LifecyclePhase::Execution,
                    component: "managed-websocket-session",
                },
            )
        });
        let reporter = super::HttpFaultReporter::authorization(
            crate::transport::websocket::native_socket_observer(
                endpoint.managed_websocket_state().schema.clone(),
                self.seed.request_telemetry().clone(),
            ),
        );
        let entry = {
            let _runtime = self.registry.runtime.enter();
            entry::Entry::new(
                endpoint.managed_websocket_state(),
                self.seed,
                socket,
                deadlines,
                Arc::clone(&scope),
            )?
        };
        self.registry.attach(
            Box::new(tracked::TrackedWork::new(
                Box::new(entry),
                Arc::clone(&scope),
                reporter,
                instrumentation,
            )),
            scope,
        )
    }
}
