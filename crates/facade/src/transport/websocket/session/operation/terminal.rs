//! Exact-generation terminal control staging after immediate logical release.

use std::sync::Arc;
use std::task::{Context, Poll};

use crate::subscription::bridge_resources::{BridgeCleanupOutcome, BridgeGeneration};
use crate::subscription::control::{SubscriptionControlFrame, WebSocketControlStaging};
use crate::subscription::worker::websocket::{WebSocketTerminalCarrier, WebSocketTerminalMessage};

use super::failure::WebSocketOperationSharedFailure;

pub(super) struct TerminalState {
    carrier: WebSocketTerminalCarrier,
    staged: bool,
    authorized: bool,
    suppressed: bool,
}

impl TerminalState {
    pub(super) fn new(carrier: WebSocketTerminalCarrier) -> Self {
        let suppressed = !carrier.permits_prior_event_drain();
        Self {
            carrier,
            staged: false,
            authorized: false,
            suppressed,
        }
    }

    pub(super) fn carrier_mut(&mut self) -> &mut WebSocketTerminalCarrier {
        &mut self.carrier
    }

    pub(super) fn stage(
        &mut self,
        id: Arc<str>,
        controls: &mut WebSocketControlStaging,
    ) -> Result<bool, WebSocketOperationSharedFailure> {
        if self.staged || self.suppressed {
            return Ok(false);
        }
        let Some(message) = self.carrier.take_message() else {
            return Err(invariant());
        };
        match message {
            WebSocketTerminalMessage::Error(response) => {
                if let Some(physical) = self.carrier.take_physical() {
                    controls.enqueue_active_error(id, physical, &response)?;
                } else {
                    controls.enqueue_pending_error(id, self.carrier.generation(), &response)?;
                }
                self.staged = true;
            }
            WebSocketTerminalMessage::Complete => {
                let physical = self.carrier.take_physical().ok_or_else(invariant)?;
                controls.enqueue_active_complete(id, physical)?;
                self.staged = true;
            }
            WebSocketTerminalMessage::Cancelled => {
                self.suppressed = true;
            }
        }
        Ok(true)
    }

    pub(super) fn authorize(
        &mut self,
        frame: &SubscriptionControlFrame,
        generation: BridgeGeneration,
    ) -> bool {
        if !self.staged || self.authorized || self.suppressed {
            return false;
        }
        if frame.websocket_generation() != Some(generation) {
            return false;
        }
        self.authorized = true;
        true
    }

    pub(super) fn suppress(&mut self) {
        self.carrier.take_message();
        self.carrier.take_physical();
        self.suppressed = true;
    }

    pub(super) fn poll_cleanup(&mut self, context: &mut Context<'_>) -> Poll<BridgeCleanupOutcome> {
        self.carrier.poll_cleanup(context)
    }

    pub(super) fn wire_done(&self) -> bool {
        self.staged || self.suppressed
    }
}

fn invariant() -> WebSocketOperationSharedFailure {
    WebSocketOperationSharedFailure::Worker(
        crate::subscription::worker::SubscriptionWorkerRuntimeError::Invariant,
    )
}
