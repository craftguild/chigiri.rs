//! Subscription configuration edits on the shared endpoint draft.

use chigiri_http::ConnectionInitAuthorizer;

use crate::component_ops;
use crate::draft::Draft;
use crate::subscription::settings::{SubscriptionLimits, SubscriptionProtocolSettings};

/// Subscription delivery, protocol, and initialization settings.
pub struct SubscriptionComponents<'a, C> {
    pub(super) draft: &'a mut Draft<C>,
}

impl<C> SubscriptionComponents<'_, C>
where
    C: Send + Sync + 'static,
{
    /// Replaces the complete endpoint and per-subscription delivery limits.
    pub fn limits(&mut self, limits: SubscriptionLimits) -> &mut Self {
        component_ops::subscription_limits(self.draft, limits);
        self
    }

    /// Replaces the complete WebSocket subscription protocol setting.
    pub fn protocol(&mut self, settings: SubscriptionProtocolSettings) -> &mut Self {
        component_ops::subscription_protocol(self.draft, settings);
        self
    }

    /// Replaces the WebSocket connection-initialization authorizer.
    pub fn connection_init_authorizer(
        &mut self,
        authorizer: impl ConnectionInitAuthorizer,
    ) -> &mut Self {
        component_ops::connection_init_authorizer(self.draft, authorizer);
        self
    }
}
