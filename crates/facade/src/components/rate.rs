//! Connect Stage A/B provider and final observer to the same slot.

use std::sync::Arc;

use chigiri_http::TransportAdmission;
use chigiri_policy::RateLimitObserverBinding;
use chigiri_runtime::Plugin;

use crate::draft::Draft;
use crate::rate::{OperationRateComponent, TransportRateComponent};

/// Transport Stage A and Operation Stage B are independent slots.
pub struct RateLimitComponents<'a, C> {
    pub(super) draft: &'a mut Draft<C>,
}

impl<C> RateLimitComponents<'_, C>
where
    C: Send + Sync + 'static,
{
    /// Replaces the current Stage A limiter; the last setting wins.
    pub fn stage_a(&mut self, admission: impl TransportRateComponent) -> &mut Self {
        let configuration = admission.configuration();
        install_stage_a(self.draft, admission);
        self.draft.stage_a_rate_limit = Some(configuration);
        self
    }

    /// Replaces the current Stage B limiter; the last setting wins.
    pub fn stage_b(&mut self, plugin: impl OperationRateComponent<C>) -> &mut Self {
        let configuration = plugin.configuration();
        install_stage_b(self.draft, plugin);
        self.draft.stage_b_rate_limit = Some(configuration);
        self
    }

    /// Injects a third-party provider through the built-in Stage A admission bridge.
    pub fn stage_a_provider(
        &mut self,
        admission: impl TransportAdmission + RateLimitObserverBinding,
    ) -> &mut Self {
        install_stage_a(self.draft, admission);
        self.draft.stage_a_rate_limit = None;
        self
    }

    /// Injects a third-party provider through the built-in Stage B plugin bridge.
    pub fn stage_b_provider(
        &mut self,
        plugin: impl Plugin<C> + RateLimitObserverBinding,
    ) -> &mut Self {
        install_stage_b(self.draft, plugin);
        self.draft.stage_b_rate_limit = None;
        self
    }
}

fn install_stage_a<C, A>(draft: &mut Draft<C>, admission: A)
where
    C: Send + Sync + 'static,
    A: TransportAdmission + RateLimitObserverBinding,
{
    draft.mark_change();
    let admission = Arc::new(admission);
    admission.set_error_observer(draft.error_observer.clone());
    draft.stage_a_observer = Some(admission.clone());
    draft.stage_a = Some(admission);
}

fn install_stage_b<C, P>(draft: &mut Draft<C>, plugin: P)
where
    C: Send + Sync + 'static,
    P: Plugin<C> + RateLimitObserverBinding,
{
    draft.mark_change();
    let plugin = Arc::new(plugin);
    plugin.set_error_observer(draft.error_observer.clone());
    draft.stage_b_observer = Some(plugin.clone());
    draft.stage_b = Some(plugin);
}
