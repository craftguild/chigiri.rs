//! Deferred construction of the one operation-local bounded event outbox.

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::SubscriptionTransport;
use chigiri_runtime::SubscriptionOverflow;

use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::outbox::{SubscriptionOutbox, SubscriptionOutboxSignal};
use crate::subscription::settings::ValidatedSubscriptionSettings;

/// Move-only endpoint configuration used after the validated operation kind is known.
pub(in crate::transport::websocket::session) struct WebSocketOperationDeliverySeed {
    settings: ValidatedSubscriptionSettings,
    bytes: EndpointByteOwner,
    send_timeout: Duration,
}

impl WebSocketOperationDeliverySeed {
    pub(in crate::transport::websocket::session) fn new(
        settings: ValidatedSubscriptionSettings,
        bytes: EndpointByteOwner,
    ) -> Self {
        let send_timeout = settings.limits().send_no_progress_timeout;
        Self {
            settings,
            bytes,
            send_timeout,
        }
    }

    pub(super) fn build(
        &self,
        overflow: SubscriptionOverflow,
        operation_id: Arc<str>,
    ) -> Result<SubscriptionOutbox, SubscriptionOutboxSignal> {
        SubscriptionOutbox::new(
            &self.settings,
            self.bytes.clone(),
            overflow,
            SubscriptionTransport::WebSocket,
            Some(operation_id),
        )
    }

    pub(super) const fn send_timeout(&self) -> Duration {
        self.send_timeout
    }
}
