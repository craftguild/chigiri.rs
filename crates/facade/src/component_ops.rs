//! Shared leaf operations for convenience builders and typed editors.

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{ConnectionInitAuthorizer, OperationSourceResolver, ResponseHook};
use chigiri_runtime::{DocumentCacheLimits, ExecutionLimits, Plugin};

use crate::draft::Draft;
use crate::subscription::settings::{SubscriptionLimits, SubscriptionProtocolSettings};

pub(crate) fn execution_limits<C: Send + Sync + 'static>(
    draft: &mut Draft<C>,
    limits: ExecutionLimits,
) {
    Draft::<C>::set(&mut draft.sequence, &mut draft.limits, limits);
}

pub(crate) fn document_cache<C: Send + Sync + 'static>(
    draft: &mut Draft<C>,
    limits: DocumentCacheLimits,
) {
    Draft::<C>::set(&mut draft.sequence, &mut draft.cache, limits);
}

pub(crate) fn body_limit<C: Send + Sync + 'static>(draft: &mut Draft<C>, bytes: usize) {
    Draft::<C>::set(&mut draft.sequence, &mut draft.body_limit, bytes);
}

pub(crate) fn response_byte_limit<C: Send + Sync + 'static>(draft: &mut Draft<C>, bytes: usize) {
    Draft::<C>::set(&mut draft.sequence, &mut draft.response_limit, bytes);
}

pub(crate) fn execution_timeout<C: Send + Sync + 'static>(draft: &mut Draft<C>, timeout: Duration) {
    Draft::<C>::set(&mut draft.sequence, &mut draft.execution_timeout, timeout);
}

pub(crate) fn subscription_limits<C: Send + Sync + 'static>(
    draft: &mut Draft<C>,
    limits: SubscriptionLimits,
) {
    Draft::<C>::set(&mut draft.sequence, &mut draft.subscription_limits, limits);
}

pub(crate) fn subscription_protocol<C: Send + Sync + 'static>(
    draft: &mut Draft<C>,
    settings: SubscriptionProtocolSettings,
) {
    Draft::<C>::set(
        &mut draft.sequence,
        &mut draft.subscription_protocol,
        settings,
    );
}

pub(crate) fn connection_init_authorizer<C: Send + Sync + 'static>(
    draft: &mut Draft<C>,
    authorizer: impl ConnectionInitAuthorizer,
) {
    Draft::<C>::set(
        &mut draft.sequence,
        &mut draft.connection_init_authorizer,
        Arc::new(authorizer),
    );
}

pub(crate) fn operation_source<C: Send + Sync + 'static>(
    draft: &mut Draft<C>,
    source: impl OperationSourceResolver,
) {
    draft.mark_change();
    draft.source = Some(Arc::new(source));
}

pub(crate) fn push_plugin<C: Send + Sync + 'static>(draft: &mut Draft<C>, plugin: impl Plugin<C>) {
    draft.mark_change();
    draft.plugins.push(Arc::new(plugin));
}

pub(crate) fn response_hook<C, S>(draft: &mut Draft<C>, hook: impl ResponseHook<S>)
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    draft.mark_change();
    draft.response_hook = Some(crate::transport::erase_hook(hook));
}
