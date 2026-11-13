//! Parse, validation, selection, and variable coercion shared by every path.

use std::sync::Arc;

use chigiri_core::executable::{InputCoercionBudget, coerce_variables_with_budget};
use chigiri_core::executor::PreparedOperation;
use chigiri_core::language::ast::OperationType;
use chigiri_core::{GraphQlError, GraphQlRequest, GraphQlResponse, Source};

use super::PreparedRequestPlan;
use crate::diagnostics::DiagnosticPhase;
use crate::execution::{diagnostics_response, operation_identity, response_code, selection_error};
use crate::{ExecutableSchema, IntrospectionPolicy};

#[derive(Clone, Copy)]
pub(crate) enum PreparationPolicy {
    Finite,
    QueryOnly,
    Subscription,
    OneShot,
    WebSocket,
}

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    pub(crate) fn prepare_request_plan<T>(
        &self,
        request: &GraphQlRequest,
        policy: PreparationPolicy,
        component: &'static str,
        finish: impl FnOnce(PreparedRequestPlan) -> Result<T, Box<GraphQlResponse>>,
    ) -> Result<T, Box<GraphQlResponse>> {
        let scope = request.request_telemetry().and_then(|telemetry| {
            telemetry.instrumentation_child(
                crate::instrumentation::InstrumentationKind::OperationResolution,
            )
        });
        let result = match &scope {
            Some(scope) => {
                scope.run(|| self.prepare_request_plan_inner(request, policy, component, finish))
            }
            None => self.prepare_request_plan_inner(request, policy, component, finish),
        };
        if let Some(scope) = scope {
            scope.finish(if result.is_ok() {
                chigiri_core::LifecycleResult::Succeeded
            } else {
                chigiri_core::LifecycleResult::Rejected
            });
        }
        result
    }

    fn prepare_request_plan_inner<T>(
        &self,
        request: &GraphQlRequest,
        policy: PreparationPolicy,
        component: &'static str,
        finish: impl FnOnce(PreparedRequestPlan) -> Result<T, Box<GraphQlResponse>>,
    ) -> Result<T, Box<GraphQlResponse>> {
        let mut phase = Some(self.inner.diagnostics.start_phase(
            chigiri_core::LifecyclePhase::DocumentPreparation,
            component,
            request.request_telemetry(),
        ));
        let source = Source::new("<request>", request.query.clone());
        let document = self
            .validated_document(request, &source)
            .map_err(|response| reject(&mut phase, response))?;
        let selected = document
            .select_operation(request.operation_name.as_deref())
            .map_err(selection_error)
            .map_err(Box::new)
            .map_err(|response| reject(&mut phase, response))?;
        let kind = selected.operation.operation_type;
        policy
            .check(self, kind)
            .map_err(|response| reject(&mut phase, response))?;
        let input_budget = self
            .inner
            .resolvers
            .input_coercion_limits()
            .map(|(nodes, bytes)| InputCoercionBudget::new(nodes, bytes));
        let variables = coerce_variables_with_budget(
            &self.inner.semantic_schema,
            &selected.operation.variable_definitions,
            request.variables.clone(),
            &self.inner.scalars,
            &self.inner.limits,
            input_budget.as_ref(),
        );
        let Some(variables) = variables.variables else {
            let response = diagnostics_response(variables.diagnostics, &source, "BAD_USER_INPUT");
            return Err(reject(&mut phase, Box::new(response)));
        };
        let identity = operation_identity(
            self.inner.info.schema_digest(),
            request,
            selected
                .operation
                .name
                .as_ref()
                .map(|name| name.value.as_str()),
        )
        .map_err(|response| reject(&mut phase, response))?;
        let identity = Arc::new(identity);
        let operation = PreparedOperation::new(
            Arc::clone(&self.inner.semantic_schema),
            document,
            request.operation_name.clone(),
            Arc::new(variables),
            Arc::clone(&self.inner.scalars),
            Arc::new(source),
            self.inner.limits,
            self.inner.introspection == IntrospectionPolicy::Enabled,
            Arc::clone(&identity),
            request.catalog_cost_hint(),
        )
        .map_err(selection_error)
        .map_err(Box::new)
        .map_err(|response| reject(&mut phase, response))?;
        let prepared = PreparedRequestPlan {
            operation,
            identity: Arc::clone(&identity),
            input_budget,
            kind,
        };
        match finish(prepared) {
            Ok(output) => {
                phase.take().expect("preparation phase is present").finish(
                    chigiri_core::LifecycleResult::Succeeded,
                    Some(&identity),
                    None,
                );
                Ok(output)
            }
            Err(response) => Err(reject(&mut phase, response)),
        }
    }
}

impl PreparationPolicy {
    fn check<C: 'static>(
        self,
        schema: &ExecutableSchema<C>,
        kind: OperationType,
    ) -> Result<(), Box<GraphQlResponse>> {
        let error = match self {
            Self::Finite | Self::QueryOnly if kind == OperationType::Subscription => Some((
                "Subscription operations require a streaming execution path",
                "OPERATION_NOT_SUPPORTED",
            )),
            Self::QueryOnly if kind != OperationType::Query => Some((
                "GET requests may only execute query operations",
                "METHOD_NOT_ALLOWED",
            )),
            Self::Subscription
                if kind != OperationType::Subscription
                    || schema
                        .inner
                        .subscription
                        .as_ref()
                        .and_then(|dispatch| dispatch.subscription_root())
                        .is_none() =>
            {
                Some((
                    "A bound subscription operation is required",
                    "OPERATION_NOT_SUPPORTED",
                ))
            }
            Self::OneShot if kind == OperationType::Subscription => Some((
                "WebSocket one-shot execution requires a query or mutation",
                "OPERATION_NOT_SUPPORTED",
            )),
            Self::WebSocket
                if kind == OperationType::Subscription
                    && schema
                        .inner
                        .subscription
                        .as_ref()
                        .and_then(|dispatch| dispatch.subscription_root())
                        .is_none() =>
            {
                Some((
                    "A bound subscription operation is required",
                    "OPERATION_NOT_SUPPORTED",
                ))
            }
            _ => None,
        };
        match error {
            Some((message, code)) => Err(Box::new(GraphQlResponse::request_error(
                GraphQlError::new(message).code(code),
            ))),
            None => Ok(()),
        }
    }
}

fn reject(
    phase: &mut Option<DiagnosticPhase<'_>>,
    response: Box<GraphQlResponse>,
) -> Box<GraphQlResponse> {
    phase.take().expect("preparation phase is present").finish(
        chigiri_core::LifecycleResult::Rejected,
        None,
        response_code(&response),
    );
    response
}
