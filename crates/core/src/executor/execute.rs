//! Entry point connecting operation middleware to root selection execution.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::executable::{
    CoercedVariables, CustomScalarRegistry, SelectedOperation, ValidatedDocument,
};
use crate::instrumentation::InstrumentationKind;
use crate::language::ast::OperationType;
use crate::schema::SemanticSchema;
use crate::{
    ExecutionLimits, GraphQlResponse, LifecycleObserver, LifecyclePhase, LifecycleResult,
    OperationIdentity, Source, Value,
};

use super::engine::Engine;
use super::lifecycle::{AdmissionLifecycle, PhaseLifecycle};
use super::{
    BoxFuture, FieldDispatch, InternalErrorHandler, OperationContext, OperationNext, Plugin,
    PreparedOperation,
};

/// Executes one immutable validated operation without parsing or coercing variables again.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn execute_prepared_operation<'a, C>(
    prepared: &'a PreparedOperation,
    dispatch: &'a dyn FieldDispatch<C>,
    plugins: &'a [Arc<dyn Plugin<C>>],
    context: &'a C,
    internal_errors: Option<&'a dyn InternalErrorHandler>,
    lifecycle: Option<&'a dyn crate::LifecycleObserver>,
    telemetry: Option<&'a crate::RequestTelemetry>,
    deadlines: Option<&'a dyn crate::DeadlineFactory>,
    input_budget: Option<&'a crate::executable::InputCoercionBudget>,
) -> Result<BoxFuture<'a, GraphQlResponse>, crate::executable::OperationSelectionError>
where
    C: Send + Sync + 'static,
{
    let selected = prepared.selected()?;
    Ok(execute_operation(
        &prepared.schema,
        &prepared.document,
        selected,
        &prepared.variables,
        &prepared.scalars,
        dispatch,
        plugins,
        context,
        &prepared.source,
        prepared.limits,
        prepared.introspection_enabled,
        &prepared.identity,
        internal_errors,
        lifecycle,
        prepared.catalog_cost_hint,
        telemetry,
        deadlines,
        input_budget,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn execute_operation<'a, C>(
    schema: &'a SemanticSchema,
    document: &'a ValidatedDocument,
    selected: SelectedOperation<'a>,
    variables: &'a CoercedVariables,
    scalars: &'a CustomScalarRegistry,
    dispatch: &'a dyn FieldDispatch<C>,
    plugins: &'a [Arc<dyn Plugin<C>>],
    context: &'a C,
    source: &'a Source,
    limits: ExecutionLimits,
    introspection_enabled: bool,
    identity: &'a OperationIdentity,
    internal_errors: Option<&'a dyn InternalErrorHandler>,
    lifecycle: Option<&'a dyn LifecycleObserver>,
    catalog_cost_hint: Option<u64>,
    telemetry: Option<&'a crate::RequestTelemetry>,
    deadlines: Option<&'a dyn crate::DeadlineFactory>,
    input_budget: Option<&'a crate::executable::InputCoercionBudget>,
) -> BoxFuture<'a, GraphQlResponse>
where
    C: Send + Sync + 'static,
{
    Box::pin(async move {
        let operation = selected.operation;
        let engine = Engine::new(
            schema,
            document,
            variables,
            scalars,
            input_budget,
            dispatch,
            plugins,
            context,
            source,
            limits,
            introspection_enabled,
            identity,
            telemetry,
            internal_errors,
        );
        let directive_sources = super::directive::operation_sources(document, operation);
        let directives = match super::directive::coerce(&engine, directive_sources, &[]) {
            Ok(directives) => directives,
            Err(errors) => {
                if let Some(response) = engine.output_byte_failure() {
                    return response;
                }
                return GraphQlResponse {
                    data: None,
                    errors,
                    extensions: crate::Object::new(),
                };
            }
        };
        let cost_fields = match super::cost_metadata::collect_operation_cost_fields_bounded(
            schema,
            document,
            selected,
            variables,
            dispatch.runtime_byte_limit(),
        ) {
            Ok(fields) => fields,
            Err(_) => {
                return GraphQlResponse {
                    data: None,
                    errors: vec![
                        crate::GraphQlError::new("Operation metadata resource limit")
                            .code("BAD_USER_INPUT"),
                    ],
                    extensions: crate::Object::new(),
                };
            }
        };
        let operation_context = OperationContext::execution(
            context,
            operation.operation_type,
            operation.name.as_ref().map(|name| name.value.as_str()),
            variables,
            selected.complexity,
            &directives,
            Some(identity),
            &cost_fields,
            catalog_cost_hint,
            telemetry,
            deadlines,
        );
        let entered_execution = AtomicBool::new(false);
        let admission_completed = AtomicBool::new(false);
        let admission =
            AdmissionLifecycle::start(lifecycle, identity, &admission_completed, telemetry);
        let entered = &entered_execution;
        let admission_for_terminal = &admission;
        let response = OperationNext::new(plugins, move |_| {
            entered.store(true, Ordering::SeqCst);
            admission_for_terminal.succeed();
            Box::pin(async move {
                let scope = telemetry.and_then(|telemetry| {
                    telemetry.instrumentation_child(InstrumentationKind::Execution)
                });
                let finish = scope.clone();
                let work = async move {
                    let execution = PhaseLifecycle::start(
                        lifecycle,
                        LifecyclePhase::Execution,
                        identity,
                        telemetry,
                    );
                    let root = match operation.operation_type {
                        OperationType::Query => schema.roots().query(),
                        OperationType::Mutation => schema.roots().mutation(),
                        OperationType::Subscription => schema.roots().subscription(),
                    };
                    let Some(root) = root else {
                        let response = GraphQlResponse::executed(Value::Null);
                        execution.finish(LifecycleResult::Succeeded);
                        if let Some(scope) = finish {
                            scope.finish(LifecycleResult::Succeeded);
                        }
                        return response;
                    };
                    let result = engine
                        .execute_selection(
                            vec![&operation.selection_set],
                            &root.type_name,
                            &root.type_name,
                            None,
                            Vec::new(),
                            operation.operation_type == OperationType::Mutation,
                        )
                        .await;
                    let response =
                        engine
                            .output_byte_failure()
                            .unwrap_or_else(|| GraphQlResponse {
                                data: Some(if result.bubble {
                                    Value::Null
                                } else {
                                    Value::Object(result.value)
                                }),
                                errors: result.errors,
                                extensions: crate::Object::new(),
                            });
                    execution.finish(LifecycleResult::Succeeded);
                    if let Some(scope) = finish {
                        scope.finish(LifecycleResult::Succeeded);
                    }
                    response
                };
                match scope.as_ref() {
                    Some(scope) => scope.instrument(work).await,
                    None => work.await,
                }
            })
        })
        .run(operation_context)
        .await;
        if !entered_execution.load(Ordering::SeqCst) {
            admission.reject();
        }
        drop(admission);
        response
    })
}
