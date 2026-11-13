//! A releasable event-context future for deterministic managed drain tests.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_http::{
    BoxFuture, ContextFactory, PrepareError, PreparedContext, RequestContext, RequestPhase,
};
use chigiri_runtime::ExecutableSchema;
use tokio::sync::watch;

use super::super::descriptor::SCHEMA;
use super::super::dispatch::Dispatch;
use super::super::state::{AppContext, HookState, State};
use super::{Factory, Hook, RequestIds};
use crate::{Endpoint, SubscriptionLimits};

pub(in crate::subscription::driver::tests) struct EventGate {
    pub(in crate::subscription::driver::tests) entered: AtomicUsize,
    release: watch::Sender<bool>,
}

impl EventGate {
    pub(in crate::subscription::driver::tests) fn release(&self) {
        self.release.send_replace(true);
    }
}

impl Drop for EventGate {
    fn drop(&mut self) {
        self.release.send_replace(true);
    }
}

struct GatedFactory {
    inner: Factory,
    gate: Arc<EventGate>,
    finite: bool,
}

impl ContextFactory<AppContext, HookState> for GatedFactory {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<PreparedContext<AppContext, HookState>, PrepareError>> {
        Box::pin(async move {
            if matches!(request.phase(), RequestPhase::SubscriptionEvent { .. })
                || (self.finite
                    && matches!(request.phase(), RequestPhase::WebSocketOperation { .. }))
            {
                let mut release = self.gate.release.subscribe();
                self.gate.entered.fetch_add(1, Ordering::SeqCst);
                while !*release.borrow_and_update() {
                    release.changed().await.map_err(|_| {
                        PrepareError::Internal(chigiri_http::HookError::new(std::io::Error::other(
                            "fixture gate closed",
                        )))
                    })?;
                }
            }
            self.inner.create(request).await
        })
    }
}

pub(in crate::subscription::driver::tests) fn endpoint(
    probe: Arc<State>,
) -> (Endpoint<AppContext>, Arc<EventGate>) {
    endpoint_inner(probe, false)
}

pub(in crate::subscription::driver::tests) fn finite_endpoint(
    probe: Arc<State>,
) -> (Endpoint<AppContext>, Arc<EventGate>) {
    endpoint_inner(probe, true)
}

fn endpoint_inner(probe: Arc<State>, finite: bool) -> (Endpoint<AppContext>, Arc<EventGate>) {
    let (factory, gate) = factory_inner(probe.clone(), finite);
    let schema = ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(probe.clone()))
        .request_id_factory(RequestIds(probe.clone()))
        .build()
        .unwrap();
    let endpoint = Endpoint::builder(schema)
        .context_factory(factory)
        .response_hook(Hook(probe))
        .subscription_limits(SubscriptionLimits::default())
        .build()
        .unwrap();
    (endpoint, gate)
}

pub(in crate::subscription::driver::tests) fn factory(
    probe: Arc<State>,
) -> (impl ContextFactory<AppContext, HookState>, Arc<EventGate>) {
    factory_inner(probe, false)
}

fn factory_inner(probe: Arc<State>, finite: bool) -> (GatedFactory, Arc<EventGate>) {
    let (release, _) = watch::channel(false);
    let gate = Arc::new(EventGate {
        entered: AtomicUsize::new(0),
        release,
    });
    (
        GatedFactory {
            inner: Factory(probe),
            gate: gate.clone(),
            finite,
        },
        gate,
    )
}
