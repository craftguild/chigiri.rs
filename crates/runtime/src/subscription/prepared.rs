//! Validated operation ownership and exactly-once generated source setup.

use std::sync::Arc;

use chigiri_core::executor::{PreparedSubscriptionPlan, prepare_subscription_plan};
use chigiri_core::{GraphQlRequest, GraphQlResponse, OperationIdentity, RequestTelemetry};

use crate::ExecutableSchema;
use crate::execution::{PreparationPolicy, PreparedRequestPlan};

use super::session::SubscriptionSession;

/// Validated immutable input awaiting a freshly prepared setup context.
#[doc(hidden)]
pub struct PreparedSubscription<C> {
    pub(super) schema: ExecutableSchema<C>,
    pub(super) plan: PreparedSubscriptionPlan,
    pub(super) request: Arc<GraphQlRequest>,
    pub(super) setup_telemetry: Option<Box<RequestTelemetry>>,
    pub(super) identity: Arc<OperationIdentity>,
    pub(super) observation_prepared: bool,
    pub(super) observation: Option<super::observation::SubscriptionObservationReceipt>,
}

/// Source initialization either establishes ownership or returns a safe response.
#[doc(hidden)]
pub enum SubscriptionStart<C> {
    /// A source is established and the setup application context is no longer borrowed.
    Ready(Box<SubscriptionSession<C>>),
    /// Initialization or policy rejected the subscription before any source is delivered.
    Response(GraphQlResponse),
}

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    /// Validates and coerces a subscription before application source initialization.
    #[doc(hidden)]
    pub fn prepare_subscription(
        &self,
        mut request: GraphQlRequest,
    ) -> Result<PreparedSubscription<C>, Box<GraphQlResponse>> {
        if request.request_telemetry().is_none() {
            request.telemetry = Some(self.inner.diagnostics.request_telemetry(None));
        }
        let prepared = self.prepare_request_plan(
            &request,
            PreparationPolicy::Subscription,
            "subscription",
            Ok,
        )?;
        PreparedSubscription::from_plan(self.clone(), prepared, Arc::new(request))
    }
}

impl<C: Send + Sync + 'static> PreparedSubscription<C> {
    pub(crate) fn from_plan(
        schema: ExecutableSchema<C>,
        prepared: PreparedRequestPlan,
        mut request: Arc<GraphQlRequest>,
    ) -> Result<Self, Box<GraphQlResponse>> {
        let PreparedRequestPlan {
            operation,
            identity,
            input_budget,
            kind: _,
        } = prepared;
        let plan = prepare_subscription_plan(
            &operation,
            schema.inner.resolvers.runtime_byte_limit(),
            input_budget.as_ref(),
            Some(&schema.inner.diagnostics),
            request.request_telemetry(),
        )?;
        let setup_telemetry = request
            .request_telemetry()
            .filter(|telemetry| telemetry.propagation_context().is_some())
            .cloned()
            .map(Box::new);
        if let Some(telemetry) = &setup_telemetry {
            Arc::get_mut(&mut request)
                .expect("subscription preparation owns its request before sharing")
                .telemetry = Some(telemetry.retained_instrumentation(usize::MAX));
        }
        Ok(Self {
            schema,
            plan,
            request,
            setup_telemetry,
            identity,
            observation_prepared: false,
            observation: None,
        })
    }
}
