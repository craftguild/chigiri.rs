//! One built endpoint's validated subscription graph and shared resource owners.

use std::sync::Arc;

use chigiri_http::ConnectionInitAuthorizer;

use super::bridge_resources::{BridgeResourceError, BridgeResourceOwner};
use super::settings::ValidatedSubscriptionSettings;

pub(crate) struct SubscriptionState {
    pub(crate) settings: ValidatedSubscriptionSettings,
    pub(crate) resources: BridgeResourceOwner,
    pub(crate) authorizer: Arc<dyn ConnectionInitAuthorizer>,
}

impl SubscriptionState {
    pub(crate) fn new(
        settings: ValidatedSubscriptionSettings,
        authorizer: Arc<dyn ConnectionInitAuthorizer>,
    ) -> Result<Self, BridgeResourceError> {
        Ok(Self {
            resources: BridgeResourceOwner::new(&settings)?,
            settings,
            authorizer,
        })
    }
}
