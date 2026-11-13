//! Exact-generation terminal identity within the one bounded control queue.

use std::sync::Arc;

use chigiri_http::{
    __encode_rejected_websocket_operation_error, __encode_subscription_control,
    RejectedWebSocketOperationId, SubscriptionControl, SubscriptionTransport,
};
use chigiri_runtime::GraphQlResponse;

use super::{SubscriptionControlSignal, WebSocketControlClass, WebSocketControlStaging};
#[cfg(any(test, feature = "subscription-worker"))]
use crate::subscription::bridge_resources::BridgeGeneration;
use crate::subscription::bridge_resources::WebSocketOperationPhysicalLease;

pub(crate) struct WebSocketControlOperation {
    id: Arc<str>,
    #[cfg(any(test, feature = "subscription-worker"))]
    generation: Option<BridgeGeneration>,
    _physical: Option<WebSocketOperationPhysicalLease>,
}

impl WebSocketControlOperation {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) const fn generation(&self) -> Option<BridgeGeneration> {
        self.generation
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    fn active(id: Arc<str>, physical: WebSocketOperationPhysicalLease) -> Self {
        Self {
            id,
            generation: Some(physical.generation()),
            _physical: Some(physical),
        }
    }
}

impl WebSocketControlStaging {
    /// Queues an unowned operation completion without permitting eviction.
    pub fn enqueue_complete(&mut self, id: &str) -> Result<(), SubscriptionControlSignal> {
        self.validate_id(id)?;
        self.enqueue_operation_complete(WebSocketControlOperation {
            id: Arc::from(id),
            #[cfg(any(test, feature = "subscription-worker"))]
            generation: None,
            _physical: None,
        })
    }

    /// Queues an unowned admission error without permitting eviction.
    pub fn enqueue_error(
        &mut self,
        id: &str,
        response: &GraphQlResponse,
    ) -> Result<(), SubscriptionControlSignal> {
        self.validate_id(id)?;
        self.enqueue_operation_error(
            WebSocketControlOperation {
                id: Arc::from(id),
                #[cfg(any(test, feature = "subscription-worker"))]
                generation: None,
                _physical: None,
            },
            response,
        )
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn enqueue_active_complete(
        &mut self,
        id: Arc<str>,
        physical: WebSocketOperationPhysicalLease,
    ) -> Result<(), SubscriptionControlSignal> {
        self.enqueue_operation_complete(WebSocketControlOperation::active(id, physical))
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn enqueue_active_error(
        &mut self,
        id: Arc<str>,
        physical: WebSocketOperationPhysicalLease,
        response: &GraphQlResponse,
    ) -> Result<(), SubscriptionControlSignal> {
        self.enqueue_operation_error(WebSocketControlOperation::active(id, physical), response)
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn enqueue_pending_error(
        &mut self,
        id: Arc<str>,
        generation: BridgeGeneration,
        response: &GraphQlResponse,
    ) -> Result<(), SubscriptionControlSignal> {
        self.enqueue_operation_error(
            WebSocketControlOperation {
                id,
                generation: Some(generation),
                _physical: None,
            },
            response,
        )
    }

    fn enqueue_operation_complete(
        &mut self,
        operation: WebSocketControlOperation,
    ) -> Result<(), SubscriptionControlSignal> {
        self.validate_id(operation.id())?;
        let id = Arc::clone(&operation.id);
        self.enqueue_dynamic(
            WebSocketControlClass::Priority,
            Some(operation),
            |maximum| {
                __encode_subscription_control(
                    SubscriptionTransport::WebSocket,
                    SubscriptionControl::Complete {
                        websocket_operation_id: Some(&id),
                    },
                    maximum,
                )
            },
        )
    }

    fn enqueue_operation_error(
        &mut self,
        operation: WebSocketControlOperation,
        response: &GraphQlResponse,
    ) -> Result<(), SubscriptionControlSignal> {
        self.validate_id(operation.id())?;
        let id = Arc::clone(&operation.id);
        self.enqueue_dynamic(
            WebSocketControlClass::Priority,
            Some(operation),
            |maximum| {
                __encode_subscription_control(
                    SubscriptionTransport::WebSocket,
                    SubscriptionControl::Error {
                        websocket_operation_id: Some(&id),
                        response,
                        dropped_total: None,
                    },
                    maximum,
                )
            },
        )
    }

    /// Queues the dedicated bounded error path without copying an oversized ID.
    pub fn enqueue_rejected_operation_error(
        &mut self,
        id: &RejectedWebSocketOperationId,
        response: &GraphQlResponse,
    ) -> Result<(), SubscriptionControlSignal> {
        self.enqueue_dynamic(WebSocketControlClass::Priority, None, |maximum| {
            __encode_rejected_websocket_operation_error(id, response, maximum)
        })
    }

    /// Cancels wholly unsent controls when the client completes this wire ID.
    /// This is cancellation, never overflow eviction or a delivery-drop count.
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn discard_operation(&mut self, id: &str) -> Result<(), SubscriptionControlSignal> {
        self.discard_matching(|operation| operation.id() == id)
    }

    /// Discards only one old generation, never a replacement using the same ID.
    #[cfg(any(test, feature = "subscription-worker"))]
    pub(crate) fn discard_generation(
        &mut self,
        generation: BridgeGeneration,
    ) -> Result<(), SubscriptionControlSignal> {
        self.discard_matching(|operation| operation.generation() == Some(generation))
    }

    #[cfg(any(test, feature = "subscription-worker"))]
    fn discard_matching(
        &mut self,
        discard: impl Fn(&WebSocketControlOperation) -> bool,
    ) -> Result<(), SubscriptionControlSignal> {
        self.ensure_live()?;
        let mut accounting = self.accounting;
        let mut failure = None;
        self.queue.retain(|queued| {
            if queued.operation.as_ref().is_some_and(&discard) {
                if let Err(signal) = accounting.remove(queued.frame.len()) {
                    failure.get_or_insert(signal);
                }
                false
            } else {
                true
            }
        });
        self.accounting = accounting;
        match failure {
            Some(signal) => Err(self.fail(signal)),
            None => Ok(()),
        }
    }

    fn validate_id(&mut self, id: &str) -> Result<(), SubscriptionControlSignal> {
        self.ensure_live()?;
        if id.is_empty() || id.len() > 256 {
            return Err(self.fail(SubscriptionControlSignal::InvalidTransportArguments));
        }
        Ok(())
    }
}
