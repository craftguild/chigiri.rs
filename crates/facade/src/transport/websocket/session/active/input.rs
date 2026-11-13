//! Authoritative admission and generation invalidation before new worker creation.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_runtime::{GraphQlError, GraphQlResponse};

use super::super::operation::{
    WebSocketOperationDeliverySeed, WebSocketOperationSharedFailure, WebSocketOperationSlot,
};
use super::super::{WebSocketInbound, WebSocketIo, WebSocketIoError};
use super::ActiveConnection;
use crate::subscription::bridge_resources::BridgeResourceError;
use crate::subscription::driver::input::{WebSocketOperationIngress, metadata_graphql_response};
use crate::subscription::failure::{SubscriptionFailureClass, SubscriptionFailureReporter};
use crate::subscription::worker::websocket::{
    WebSocketSetupDeadline, WebSocketTerminalCarrier, WebSocketValidationWorker,
};
use crate::transport::websocket::{InitializedWebSocketInput, WebSocketConnectionClose};

impl<C: Send + Sync + 'static, S: WebSocketIo> ActiveConnection<C, S> {
    pub(super) fn poll_input(
        &mut self,
        context: &mut Context<'_>,
    ) -> Result<bool, WebSocketOperationSharedFailure> {
        let message =
            match Pin::new(self.socket.as_mut().expect("live socket")).poll_receive(context) {
                Poll::Pending => return Ok(false),
                Poll::Ready(Err(error)) => {
                    self.io_failure(error);
                    return Ok(true);
                }
                Poll::Ready(Ok(None)) => {
                    self.close(WebSocketConnectionClose::internal(), false);
                    return Ok(true);
                }
                Poll::Ready(Ok(Some(message))) => message,
            };
        let parts = self.parts.as_mut().expect("live initialized connection");
        let input = match message {
            WebSocketInbound::Text(text) => parts.receive_text(&text),
            WebSocketInbound::Binary => Err(parts.receive_binary()),
        };
        match input {
            Err(close) => self.close(close, true),
            Ok(InitializedWebSocketInput::Subscribe(subscribe)) => self.subscribe(subscribe)?,
            Ok(InitializedWebSocketInput::Complete(id)) => {
                self.invalidate_id(id.as_str())?;
            }
            Ok(InitializedWebSocketInput::RejectedOperationId(id)) => {
                let response = GraphQlResponse::request_error(
                    GraphQlError::new("WebSocket operation ID exceeds its limit")
                        .code("BAD_USER_INPUT"),
                );
                self.parts
                    .as_mut()
                    .expect("live controls")
                    .controls
                    .enqueue_rejected_operation_error(&id, &response)?;
            }
            Ok(
                InitializedWebSocketInput::ControlQueued | InitializedWebSocketInput::PongObserved,
            ) => {}
        }
        Ok(true)
    }

    fn invalidate_id(&mut self, id: &str) -> Result<(), WebSocketOperationSharedFailure> {
        for index in 0..self.slots.len() {
            if self.slots[index]
                .as_ref()
                .is_some_and(|slot| slot.wire_id() == Some(id))
            {
                let transition = self.slots[index]
                    .as_mut()
                    .expect("matching slot")
                    .cancel(&mut self.parts.as_mut().expect("live controls").controls)?;
                self.consume_transition(index, transition);
            }
        }
        // Also covers completed operations whose terminal-only permission has
        // moved completely into the original bounded control staging queue.
        self.parts
            .as_mut()
            .expect("live controls")
            .controls
            .discard_operation(id)?;
        Ok(())
    }

    fn subscribe(
        &mut self,
        subscribe: chigiri_http::WebSocketSubscribe,
    ) -> Result<(), WebSocketOperationSharedFailure> {
        let (id, envelope) = subscribe.into_parts();
        let id: Arc<str> = Arc::from(id.into_string());
        let parts = self.parts.as_ref().expect("live initialized connection");
        // Physical reservation and its rollback can wake an owner. Propose it
        // outside the endpoint gate, then authorize before telemetry/user code.
        let proposed = parts.operation_admission().begin(Arc::clone(&id));
        if !self.configuration.execution.authorize_start() {
            drop(proposed);
            self.sync_shutdown();
            return Ok(());
        }
        let telemetry = self
            .configuration
            .execution
            .schema
            .request_telemetry(parts.request.telemetry().trace_id().cloned());
        let failure_telemetry = match parts.request.instrumentation() {
            Some(captured) => telemetry.clone().with_instrumentation_scope(
                captured.retain_within(0).handoff_scope(
                    chigiri_runtime::instrumentation::InstrumentationKind::SubscriptionSetup,
                ),
            ),
            None => telemetry.clone(),
        };
        let failure = SubscriptionFailureReporter::new(
            self.configuration.execution.schema.clone(),
            failure_telemetry,
            "subscription-websocket-supervision",
        );
        let pending = match proposed {
            Ok(pending) => pending,
            Err(BridgeResourceError::DuplicateId) => {
                self.close(WebSocketConnectionClose::duplicate_operation(), true);
                return Ok(());
            }
            Err(BridgeResourceError::Capacity) => {
                let error = BridgeResourceError::Capacity;
                let response = failure.response(SubscriptionFailureClass::Limit, &error);
                self.parts
                    .as_mut()
                    .expect("live controls")
                    .controls
                    .enqueue_error(&id, &response)?;
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let generation = pending.generation();
        let deadlines = parts.deadlines;
        let setup = WebSocketSetupDeadline::new(
            deadlines,
            deadlines.sleep(
                self.configuration
                    .execution
                    .subscriptions
                    .limits()
                    .setup_timeout,
            ),
        );
        // No await, socket poll, or application worker creation can interleave
        // authoritative acceptance with invalidating every old unsent frame.
        self.invalidate_id(&id)?;
        let index = self.slots.iter().position(Option::is_none).ok_or(
            WebSocketOperationSharedFailure::Resource(BridgeResourceError::Invariant),
        )?;
        let parts = self.parts.as_ref().expect("live initialized connection");
        let request = Arc::clone(&parts.request);
        let execution = Arc::clone(&self.configuration.execution);
        let lifecycle = self.configuration.lifecycle.clone();
        let initialization = parts.initialization_payload.clone();
        let ingress = request.inspect_transport(|transport| {
            WebSocketOperationIngress::capture(
                execution,
                lifecycle,
                transport,
                envelope,
                telemetry,
                initialization,
            )
        });
        let delivery = WebSocketOperationDeliverySeed::new(
            self.configuration.execution.subscriptions,
            parts.owner.clone(),
        );
        let slot = match ingress {
            Ok(ingress) => {
                match WebSocketValidationWorker::new(ingress, pending, setup, failure.clone()) {
                    Ok(worker) => {
                        WebSocketOperationSlot::new(id, generation, worker, delivery, failure)?
                    }
                    Err(start) => WebSocketOperationSlot::from_terminal(
                        id,
                        start.into_terminal()?,
                        delivery,
                        failure,
                    ),
                }
            }
            Err(_) => WebSocketOperationSlot::from_terminal(
                id,
                WebSocketTerminalCarrier::rejected_pending(
                    pending,
                    metadata_graphql_response(),
                    setup,
                )?,
                delivery,
                failure,
            ),
        };
        self.slots[index] = Some(slot);
        Ok(())
    }

    pub(super) fn io_failure(&mut self, error: WebSocketIoError) {
        match error {
            WebSocketIoError::Protocol => self.close(WebSocketConnectionClose::bad_request(), true),
            WebSocketIoError::Transport => {
                self.close(WebSocketConnectionClose::internal(), false);
            }
            WebSocketIoError::Invariant => self.close_internal(),
        }
    }
}
