//! Completes one source event with fresh middleware and execution budgets.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::instrumentation::InstrumentationKind;
use crate::language::ast::OperationType;
use crate::{FieldError, FieldResult, GraphQlResponse, LifecyclePhase, LifecycleResult, Value};

use super::super::engine::{Completion, Engine};
use super::super::lifecycle::{AdmissionLifecycle, PhaseLifecycle};
use super::super::{
    BoxFuture, ExecutionFailureKind, FieldContext, FieldNext, OperationContext, OperationNext,
    ResolvedValue, SelectionMetadata, collect_fields,
};
use super::{PreparedSubscriptionRoot, SubscriptionExecutionInput};

/// Generated event resolver at the typed core/runtime boundary.
#[doc(hidden)]
pub trait SubscriptionEventDispatch<C>: Send + Sync {
    /// Converts one application event into the root field's ordinary result.
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, C>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}

/// Executes one event without re-running root source setup or root dispatch.
#[doc(hidden)]
pub fn execute_subscription_event<'a, C>(
    input: SubscriptionExecutionInput<'a, C>,
    root: &'a PreparedSubscriptionRoot,
    event: &'a dyn SubscriptionEventDispatch<C>,
) -> BoxFuture<'a, GraphQlResponse>
where
    C: Send + Sync + 'static,
{
    Box::pin(async move {
        let prepared = &root.operation;
        let engine = input.engine(prepared);
        let selected = match prepared.selected() {
            Ok(selected) => selected,
            Err(_) => {
                return invariant_response(
                    &engine,
                    root,
                    "prepared subscription operation can no longer be selected",
                );
            }
        };
        let operation = selected.operation;
        if operation.operation_type != OperationType::Subscription {
            return invariant_response(
                &engine,
                root,
                "subscription event received a non-subscription operation",
            );
        }
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
        let input_for_terminal = &input;
        let lifecycle = input.lifecycle;
        let identity = &prepared.identity;
        let telemetry = input.telemetry;
        let response = OperationNext::new(input.plugins, move |_| {
            entered_for_terminal.store(true, Ordering::SeqCst);
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
                    let response = execute_root_event(
                        engine_for_terminal,
                        input_for_terminal,
                        root,
                        selected,
                        event,
                    )
                    .await;
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
        if !entered.load(Ordering::SeqCst) {
            admission.reject();
        }
        drop(admission);
        response
    })
}

async fn execute_root_event<C>(
    engine: &Engine<'_, C>,
    input: &SubscriptionExecutionInput<'_, C>,
    root: &PreparedSubscriptionRoot,
    selected: crate::executable::SelectedOperation<'_>,
    event: &dyn SubscriptionEventDispatch<C>,
) -> GraphQlResponse
where
    C: Send + Sync + 'static,
{
    let prepared = &root.operation;
    let operation = selected.operation;
    let Some(schema_root) = prepared.schema.roots().subscription() else {
        return invariant_response(engine, root, "validated subscription root is missing");
    };
    let fields = collect_fields(
        &prepared.schema,
        &prepared.document,
        &[&operation.selection_set],
        &schema_root.type_name,
        &prepared.variables,
    );
    let [group] = fields.as_slice() else {
        return invariant_response(
            engine,
            root,
            "subscription root collection changed after setup",
        );
    };
    let field = group.fields[0];
    if schema_root.type_name != root.parent_type
        || field.name.value != root.field_name
        || group.response_key != root.response_key
    {
        return invariant_response(
            engine,
            root,
            "subscription root identity changed after setup",
        );
    }
    let Some(definition) = prepared
        .schema
        .type_by_name(&root.parent_type)
        .and_then(|ty| ty.field(&root.field_name))
    else {
        return invariant_response(
            engine,
            root,
            "prepared subscription field definition is missing",
        );
    };
    if !engine.claim_output_field(&[], &root.response_key) {
        return engine.output_byte_failure().unwrap_or_else(|| {
            invariant_response(engine, root, "subscription response-key accounting failed")
        });
    }
    let path = vec![root.response_key.as_str().into()];
    if !engine.claim_output_node() {
        return response_from_completion(
            root,
            engine.output_limit_error(
                path,
                root.locations.clone(),
                matches!(
                    definition.ty.kind,
                    crate::language::ast::TypeRefKind::NonNull(_)
                ),
                Some((&root.parent_type, &root.field_name)),
                "completion",
            ),
        );
    }
    let selection_sets = group
        .fields
        .iter()
        .filter_map(|field| field.selection_set.as_ref())
        .collect::<Vec<_>>();
    let metadata = SelectionMetadata {
        response_key: &root.response_key,
        has_subselection: !selection_sets.is_empty(),
    };
    let context = FieldContext::execution(
        input.context,
        &root.parent_type,
        &root.field_name,
        &path,
        &root.arguments,
        metadata,
        &root.directives,
        Some(&prepared.identity),
        input.telemetry,
    );
    let result = FieldNext::new(input.plugins, move |context| event.resolve_event(context))
        .run(context)
        .await;
    let completion = engine
        .complete_value(
            &definition.ty,
            result,
            selection_sets,
            path,
            root.locations.clone(),
            (&root.parent_type, &root.field_name),
        )
        .await;
    engine
        .output_byte_failure()
        .unwrap_or_else(|| response_from_completion(root, completion))
}

fn response_from_completion(
    root: &PreparedSubscriptionRoot,
    completion: Completion,
) -> GraphQlResponse {
    let data = if completion.bubble {
        Value::Null
    } else {
        let mut object = crate::Object::new();
        object.insert(root.response_key.clone(), completion.value);
        Value::Object(object)
    };
    GraphQlResponse {
        data: Some(data),
        errors: completion.errors,
        extensions: crate::Object::new(),
    }
}

fn invariant_response<C>(
    engine: &Engine<'_, C>,
    root: &PreparedSubscriptionRoot,
    message: &'static str,
) -> GraphQlResponse {
    engine.notify_execution_failure(ExecutionFailureKind::Invariant);
    GraphQlResponse {
        data: Some(Value::Null),
        errors: vec![engine.graphql_error(
            FieldError::internal(SubscriptionEventInvariant(message)),
            root.locations.clone(),
            vec![root.response_key.as_str().into()],
            Some((&root.parent_type, &root.field_name)),
            "executor",
        )],
        extensions: crate::Object::new(),
    }
}

#[derive(Debug)]
struct SubscriptionEventInvariant(&'static str);

impl fmt::Display for SubscriptionEventInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for SubscriptionEventInvariant {}
