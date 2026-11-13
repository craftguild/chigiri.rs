//! Runs subscription policy and source setup from context-free prepared metadata.

use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{FieldError, FieldResult, GraphQlResponse, LifecyclePhase, LifecycleResult};

use super::super::lifecycle::{AdmissionLifecycle, PhaseLifecycle};
use super::super::{
    ArgumentValues, BoxFuture, ExecutionFailureKind, FieldContext, FieldNext, OperationContext,
    OperationNext, ResolvedValue, SelectionMetadata,
};
use super::{
    PreparedSubscriptionOperation, PreparedSubscriptionPlan, SubscriptionExecutionInput,
    SubscriptionSetupOutcome, prepare_subscription_plan,
};

/// Generated source setup at the typed core/runtime boundary.
#[doc(hidden)]
pub trait SubscriptionSetupDispatch<C, S>: Send + Sync {
    /// Starts the application source from already-coerced root arguments.
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<S>>;
}

/// Runs context-dependent policy and source setup from a consumed prepared plan.
#[doc(hidden)]
pub fn establish_subscription_source<'a, C, S>(
    plan: PreparedSubscriptionPlan,
    input: SubscriptionExecutionInput<'a, C>,
    setup: &'a dyn SubscriptionSetupDispatch<C, S>,
) -> BoxFuture<'a, SubscriptionSetupOutcome<S>>
where
    C: Send + Sync + 'static,
    S: Send + 'static,
{
    Box::pin(async move {
        let PreparedSubscriptionPlan {
            root,
            dispatch_arguments,
            has_subselection,
        } = plan;
        let prepared = &root.operation;
        let engine = input.engine(prepared);
        let selected = match prepared.selected() {
            Ok(selected) => selected,
            Err(_) => {
                return SubscriptionSetupOutcome::Response(invariant_response(
                    &engine,
                    "prepared subscription operation can no longer be selected",
                    Vec::new(),
                    Vec::new(),
                    None,
                ));
            }
        };
        let operation = selected.operation;
        let path = vec![root.response_key.as_str().into()];
        let source_slot = Mutex::new(None);
        let operation_context = OperationContext::execution(
            input.context,
            operation.operation_type,
            operation.name.as_ref().map(|name| name.value.as_str()),
            &prepared.variables,
            selected.complexity,
            &root.operation_directives,
            Some(&prepared.identity),
            &root.cost_fields,
            prepared.catalog_cost_hint,
            input.telemetry,
            input.deadlines,
        );
        let entered = AtomicBool::new(false);
        let admission_completed = AtomicBool::new(false);
        let admission = AdmissionLifecycle::start(
            input.lifecycle,
            &prepared.identity,
            &admission_completed,
            input.telemetry,
        );
        let entered_for_terminal = &entered;
        let admission_for_terminal = &admission;
        let engine_for_terminal = &engine;
        let root_for_terminal = &root;
        let path_for_terminal = &path;
        let source_slot_for_terminal = &source_slot;
        let plugins = input.plugins;
        let context = input.context;
        let lifecycle = input.lifecycle;
        let identity = &prepared.identity;
        let telemetry = input.telemetry;
        let response = OperationNext::new(input.plugins, move |_| {
            entered_for_terminal.store(true, Ordering::SeqCst);
            admission_for_terminal.succeed();
            Box::pin(async move {
                let execution = PhaseLifecycle::start(
                    lifecycle,
                    LifecyclePhase::Execution,
                    identity,
                    telemetry,
                );
                let metadata = SelectionMetadata {
                    response_key: &root_for_terminal.response_key,
                    has_subselection,
                };
                let field_context = FieldContext::execution(
                    context,
                    &root_for_terminal.parent_type,
                    &root_for_terminal.field_name,
                    path_for_terminal,
                    &root_for_terminal.arguments,
                    metadata,
                    &root_for_terminal.directives,
                    Some(identity),
                    telemetry,
                );
                let result = FieldNext::new(plugins, move |context| {
                    Box::pin(async move {
                        match setup.subscribe(context, dispatch_arguments).await {
                            Ok(session) => match source_slot_for_terminal.lock() {
                                Ok(mut slot) => {
                                    *slot = Some(session);
                                    Ok(ResolvedValue::Null)
                                }
                                Err(_) => {
                                    engine_for_terminal
                                        .notify_execution_failure(ExecutionFailureKind::Invariant);
                                    Err(FieldError::internal(SubscriptionInvariant(
                                        "subscription source slot was poisoned",
                                    )))
                                }
                            },
                            Err(error) => Err(error),
                        }
                    })
                })
                .run(field_context)
                .await;
                let response = match result {
                    Ok(_) => GraphQlResponse {
                        data: None,
                        errors: Vec::new(),
                        extensions: crate::Object::new(),
                    },
                    Err(error) => {
                        GraphQlResponse::request_error(engine_for_terminal.graphql_error(
                            error,
                            root_for_terminal.locations.clone(),
                            path_for_terminal.clone(),
                            Some((
                                &root_for_terminal.parent_type,
                                &root_for_terminal.field_name,
                            )),
                            "resolver",
                        ))
                    }
                };
                let response = engine_for_terminal
                    .output_byte_failure()
                    .unwrap_or(response);
                execution.finish(LifecycleResult::Succeeded);
                response
            })
        })
        .run(operation_context)
        .await;
        if !entered.load(Ordering::SeqCst) {
            admission.reject();
        }
        drop(admission);
        let session = source_slot.into_inner().ok().flatten();
        if !is_empty_setup_response(&response) {
            return SubscriptionSetupOutcome::Response(response);
        }
        match session {
            Some(session) => SubscriptionSetupOutcome::Ready {
                root: Box::new(root),
                session,
            },
            None => SubscriptionSetupOutcome::Response(invariant_response(
                &engine,
                "subscription setup completed without producing a source",
                root.locations.clone(),
                path,
                Some((&root.parent_type, &root.field_name)),
            )),
        }
    })
}

/// Compatibility helper that preflights before returning the source future.
#[doc(hidden)]
pub fn prepare_subscription_root<'a, C, S>(
    prepared: &'a PreparedSubscriptionOperation,
    input: SubscriptionExecutionInput<'a, C>,
    setup: &'a dyn SubscriptionSetupDispatch<C, S>,
) -> BoxFuture<'a, SubscriptionSetupOutcome<S>>
where
    C: Send + Sync + 'static,
    S: Send + 'static,
{
    let plan = prepare_subscription_plan(
        prepared,
        input.dispatch.runtime_byte_limit(),
        input.input_budget,
        input.internal_errors,
        input.telemetry,
    );
    match plan {
        Ok(plan) => establish_subscription_source(plan, input, setup),
        Err(response) => Box::pin(async move { SubscriptionSetupOutcome::Response(*response) }),
    }
}

fn is_empty_setup_response(response: &GraphQlResponse) -> bool {
    response.data.is_none() && response.errors.is_empty() && response.extensions.is_empty()
}

fn invariant_response<C>(
    engine: &super::super::engine::Engine<'_, C>,
    message: &'static str,
    locations: Vec<crate::Location>,
    path: Vec<crate::PathSegment>,
    coordinate: Option<(&str, &str)>,
) -> GraphQlResponse {
    engine.notify_execution_failure(ExecutionFailureKind::Invariant);
    GraphQlResponse::request_error(engine.graphql_error(
        FieldError::internal(SubscriptionInvariant(message)),
        locations,
        path,
        coordinate,
        "executor",
    ))
}

#[derive(Debug)]
struct SubscriptionInvariant(&'static str);

impl fmt::Display for SubscriptionInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for SubscriptionInvariant {}
