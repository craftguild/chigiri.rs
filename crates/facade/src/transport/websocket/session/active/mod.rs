//! Exclusive input, operation, timer and writer timeline after acknowledgment.

mod clock;
mod graceful;
mod input;
mod output;
mod polling;
#[cfg(test)]
mod probe;

use std::sync::Arc;

use chigiri_http::{BoxFuture, HttpLifecycle};

use super::WebSocketIo;
use super::closing::Closing;
use super::operation::{WebSocketOperationSlot, WebSocketOperationTransition};
use super::writer::OpeningWrite;
use crate::subscription::bridge_resources::{BridgeGeneration, BridgeResourceError};
use crate::subscription::control::{WebSocketControlStaging, WebSocketWriterSchedule};
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::execution_state::SubscriptionExecutionState;
use crate::transport::websocket::InitializedWebSocketConnectionParts;
use clock::SessionClock;

pub(super) struct ActiveConfiguration<C> {
    pub(super) execution: Arc<SubscriptionExecutionState<C>>,
    pub(super) lifecycle: HttpLifecycle,
    pub(super) owner: EndpointByteOwner,
}

enum CurrentWrite {
    Control(OpeningWrite),
    Event {
        slot: usize,
        generation: BridgeGeneration,
        flushed: bool,
    },
}

pub(super) struct ActiveConnection<C: Send + Sync + 'static, S: WebSocketIo> {
    configuration: ActiveConfiguration<C>,
    socket: Option<S>,
    parts: Option<InitializedWebSocketConnectionParts>,
    slots: Vec<Option<WebSocketOperationSlot<C>>>,
    ready: Vec<bool>,
    schedule: WebSocketWriterSchedule,
    current: Option<CurrentWrite>,
    clock: SessionClock,
    heartbeat: BoxFuture<'static, ()>,
    heartbeat_due: bool,
    draining: bool,
    event_timer: Option<(u64, BoxFuture<'static, ()>)>,
    closing: Option<Closing>,
    closing_controls: Option<WebSocketControlStaging>,
    #[cfg(test)]
    before_send: Option<Box<dyn FnOnce() + Send>>,
}

impl<C: Send + Sync + 'static, S: WebSocketIo> ActiveConnection<C, S> {
    pub(super) fn new(
        configuration: ActiveConfiguration<C>,
        socket: S,
        parts: InitializedWebSocketConnectionParts,
    ) -> Result<Self, BridgeResourceError> {
        let protocol = configuration.execution.subscriptions.protocol();
        let count = protocol
            .max_active_operations_per_connection
            .checked_add(protocol.max_pending_setups_per_connection)
            .ok_or(BridgeResourceError::Invariant)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count)
            .map_err(|_| BridgeResourceError::Capacity)?;
        slots.resize_with(count, || None);
        let mut ready = Vec::new();
        ready
            .try_reserve_exact(count)
            .map_err(|_| BridgeResourceError::Capacity)?;
        ready.resize(count, false);
        let heartbeat = parts.deadlines.sleep(
            configuration
                .execution
                .subscriptions
                .limits()
                .heartbeat_interval,
        );
        Ok(Self {
            configuration,
            socket: Some(socket),
            parts: Some(parts),
            slots,
            ready,
            schedule: WebSocketWriterSchedule::default(),
            current: None,
            clock: SessionClock::new(),
            heartbeat,
            heartbeat_due: false,
            draining: false,
            event_timer: None,
            closing: None,
            closing_controls: None,
            #[cfg(test)]
            before_send: None,
        })
    }

    fn consume_transition(&mut self, index: usize, transition: WebSocketOperationTransition) {
        match transition {
            WebSocketOperationTransition::Released { id, generation } => {
                // The resource owner already released the exact wire ID. There
                // is deliberately no competing ID-to-generation authority here.
                debug_assert_eq!(
                    self.slots[index].as_ref().map(|slot| slot.generation()),
                    Some(generation)
                );
                drop(id);
            }
            WebSocketOperationTransition::Finished => {
                self.slots[index].take();
            }
            WebSocketOperationTransition::Progress => {}
        }
    }
}
