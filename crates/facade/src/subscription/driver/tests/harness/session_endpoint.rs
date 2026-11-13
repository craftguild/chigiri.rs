//! Public endpoint construction using the existing concrete execution fixture.

use std::sync::Arc;

use chigiri_runtime::{ErrorObserver, ExecutableSchema, LifecycleObserver};

use super::super::descriptor::SCHEMA;
use super::super::dispatch::Dispatch;
use super::super::state::{AppContext, State};
use super::{Factory, Hook, RequestIds};
use crate::{Endpoint, SubscriptionLimits};

pub(in crate::subscription::driver::tests) fn session_endpoint(
    probe: Arc<State>,
    limits: SubscriptionLimits,
) -> Endpoint<AppContext> {
    let schema =
        ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(Arc::clone(&probe)))
            .build()
            .unwrap();
    build(probe, limits, schema)
}

pub(in crate::subscription::driver::tests) fn observed_session_endpoint(
    probe: Arc<State>,
    lifecycle: impl LifecycleObserver,
    errors: impl ErrorObserver,
) -> Endpoint<AppContext> {
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .build()
        .unwrap();
    Endpoint::builder(schema)
        .inject(|components| {
            components
                .observability()
                .request_id_factory(RequestIds(probe.clone()))
                .lifecycle_observer(lifecycle)
                .error_observer(errors);
        })
        .context_factory(Factory(probe.clone()))
        .response_hook(Hook(probe))
        .build()
        .unwrap()
}

fn build(
    probe: Arc<State>,
    limits: SubscriptionLimits,
    schema: ExecutableSchema<AppContext>,
) -> Endpoint<AppContext> {
    Endpoint::builder(schema)
        .inject(|components| {
            components
                .observability()
                .request_id_factory(RequestIds(probe.clone()));
        })
        .context_factory(Factory(Arc::clone(&probe)))
        .response_hook(Hook(probe))
        .subscription_limits(limits)
        .build()
        .unwrap()
}
