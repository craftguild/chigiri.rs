//! Move-only Query/Mutation checkpoint for a joined WebSocket validation worker.

use std::sync::Arc;

use chigiri_core::executor::{BoxFuture, RequestContext, RequestNext};
use chigiri_core::language::ast::OperationType;
use chigiri_core::{DeadlineFactory, GraphQlRequest, GraphQlResponse, OperationIdentity};

use super::{PreparationPolicy, PreparedRequestPlan, retained_request_metadata_bytes};
use crate::ExecutableSchema;
use crate::execution::{lifecycle_result, response_code};

/// The exact one-shot operation kind fixed by full validation and selection.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OneShotOperationKind {
    /// A prepared query operation.
    Query,
    /// A prepared mutation operation.
    Mutation,
}

/// Fully validated resource-free one-shot operation awaiting active execution.
#[doc(hidden)]
pub struct PreparedOneShotOperation<C> {
    schema: ExecutableSchema<C>,
    plan: PreparedRequestPlan,
    request: Arc<GraphQlRequest>,
    kind: OneShotOperationKind,
}

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    /// Fully validates and coerces a WebSocket Query or Mutation without a context.
    #[doc(hidden)]
    pub fn prepare_one_shot_operation(
        &self,
        mut request: GraphQlRequest,
    ) -> Result<PreparedOneShotOperation<C>, Box<GraphQlResponse>> {
        if request.request_telemetry().is_none() {
            request.telemetry = Some(self.inner.diagnostics.request_telemetry(None));
        }
        let plan = self.prepare_request_plan(
            &request,
            PreparationPolicy::OneShot,
            "websocket-one-shot",
            Ok,
        )?;
        Ok(PreparedOneShotOperation::from_plan(
            self.clone(),
            plan,
            Arc::new(request),
        ))
    }
}

impl<C: Send + Sync + 'static> PreparedOneShotOperation<C> {
    pub(super) fn from_plan(
        schema: ExecutableSchema<C>,
        plan: PreparedRequestPlan,
        request: Arc<GraphQlRequest>,
    ) -> Self {
        let kind = match plan.kind {
            OperationType::Query => OneShotOperationKind::Query,
            OperationType::Mutation => OneShotOperationKind::Mutation,
            OperationType::Subscription => unreachable!("one-shot plan must not be subscription"),
        };
        Self {
            schema,
            plan,
            request,
            kind,
        }
    }

    /// Returns the operation kind fixed before active admission.
    pub const fn kind(&self) -> OneShotOperationKind {
        self.kind
    }

    /// Returns the operation identity fixed during preparation.
    pub fn identity(&self) -> &OperationIdentity {
        &self.plan.identity
    }

    /// Shares the identity allocation retained by the prepared plan.
    pub fn retained_identity(&self) -> Arc<OperationIdentity> {
        Arc::clone(&self.plan.identity)
    }

    /// Shares the decoded request without copying its retained allocations.
    pub fn retained_request(&self) -> Arc<GraphQlRequest> {
        Arc::clone(&self.request)
    }

    /// Charges the exact owned request and immutable operation metadata.
    pub fn retained_metadata_bytes(&self) -> Option<usize> {
        retained_request_metadata_bytes(
            self.plan.operation.retained_metadata_bytes()?,
            &self.request,
        )
    }

    pub(super) fn required_retained_metadata_bytes(&self) -> Option<usize> {
        self.retained_metadata_bytes()?
            .checked_sub(self.optional_trace_id_bytes())
    }

    pub(super) fn retain_instrumentation(&mut self, available: usize) -> Option<()> {
        if self.optional_trace_id_bytes() > available {
            Arc::get_mut(&mut self.request)?
                .telemetry
                .as_mut()?
                .retain_instrumentation_trace_id(available);
        }
        Some(())
    }

    fn optional_trace_id_bytes(&self) -> usize {
        self.request
            .request_telemetry()
            .filter(|telemetry| telemetry.propagation_context().is_some())
            .and_then(chigiri_core::RequestTelemetry::trace_id)
            .map_or(0, |id| id.as_str().len())
    }

    /// Consumes the checkpoint and executes it once with fresh application state.
    pub fn execute(
        self,
        context: C,
        deadlines: Option<&dyn DeadlineFactory>,
    ) -> BoxFuture<'_, GraphQlResponse> {
        Box::pin(async move {
            let Self {
                schema,
                plan,
                request,
                kind: _,
            } = self;
            let phase = schema.inner.diagnostics.start_phase(
                chigiri_core::LifecyclePhase::RequestPlugin,
                "websocket-one-shot",
                request.request_telemetry(),
            );
            let plan_ref = &plan;
            let schema_ref = &schema;
            let context_ref = &context;
            let telemetry = request.request_telemetry();
            let response = RequestNext::new(&schema.inner.plugins, move |_| {
                Box::pin(async move {
                    schema_ref
                        .run_prepared_plan(plan_ref, context_ref, telemetry, deadlines)
                        .await
                })
            })
            .run(RequestContext::new(&context, &request))
            .await;
            phase.finish(
                lifecycle_result(&response),
                Some(&plan.identity),
                response_code(&response),
            );
            response
        })
    }
}
