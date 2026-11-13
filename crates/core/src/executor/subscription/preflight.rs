//! Context-free subscription root collection, coercion, and metadata preparation.

use std::fmt;
use std::sync::Arc;

use crate::executable::InputCoercionBudget;
use crate::language::ast::OperationType;
use crate::{FieldError, FieldResult, GraphQlResponse, RequestTelemetry};

use super::super::engine::Engine;
use super::super::{
    ArgumentValues, BoxFuture, ExecutionFailureKind, FieldDispatch, InternalErrorHandler,
    ObjectScope, Plugin, ResolvedObject, ResolvedValue, collect_fields,
};
use super::{PreparedSubscriptionOperation, PreparedSubscriptionPlan, PreparedSubscriptionRoot};

/// Collects and coerces subscription root metadata without application context.
#[doc(hidden)]
pub fn prepare_subscription_plan(
    prepared: &PreparedSubscriptionOperation,
    runtime_byte_limit: Option<usize>,
    input_budget: Option<&InputCoercionBudget>,
    internal_errors: Option<&dyn InternalErrorHandler>,
    telemetry: Option<&RequestTelemetry>,
) -> Result<PreparedSubscriptionPlan, Box<GraphQlResponse>> {
    let dispatch = MetadataDispatch { runtime_byte_limit };
    let plugins: &[Arc<dyn Plugin<()>>] = &[];
    let engine = Engine::new(
        &prepared.schema,
        &prepared.document,
        &prepared.variables,
        &prepared.scalars,
        input_budget,
        &dispatch,
        plugins,
        &(),
        &prepared.source,
        prepared.limits,
        prepared.introspection_enabled,
        &prepared.identity,
        telemetry,
        internal_errors,
    );
    let selected = prepared.selected().map_err(|_| {
        Box::new(invariant_response(
            &engine,
            "prepared subscription operation can no longer be selected",
            Vec::new(),
            Vec::new(),
            None,
        ))
    })?;
    let operation = selected.operation;
    if operation.operation_type != OperationType::Subscription {
        return Err(Box::new(invariant_response(
            &engine,
            "subscription setup received a non-subscription operation",
            Vec::new(),
            Vec::new(),
            None,
        )));
    }
    let Some(schema_root) = prepared.schema.roots().subscription() else {
        return Err(Box::new(invariant_response(
            &engine,
            "validated subscription root is missing",
            Vec::new(),
            Vec::new(),
            None,
        )));
    };
    let operation_sources =
        super::super::directive::operation_sources(&prepared.document, operation);
    let operation_directives = super::super::directive::coerce(&engine, operation_sources, &[])
        .map_err(|errors| preflight_response(&engine, errors))?;
    let cost_fields = super::super::cost_metadata::collect_operation_cost_fields_bounded(
        &prepared.schema,
        &prepared.document,
        selected,
        &prepared.variables,
        runtime_byte_limit,
    )
    .map_err(|_| {
        Box::new(GraphQlResponse::request_error(
            crate::GraphQlError::new("Operation metadata resource limit").code("BAD_USER_INPUT"),
        ))
    })?;
    let fields = collect_fields(
        &prepared.schema,
        &prepared.document,
        &[&operation.selection_set],
        &schema_root.type_name,
        &prepared.variables,
    );
    let [group] = fields.as_slice() else {
        return Err(Box::new(invariant_response(
            &engine,
            "subscription root collection did not produce exactly one field",
            Vec::new(),
            Vec::new(),
            None,
        )));
    };
    let field = group.fields[0];
    let Some(definition) = prepared
        .schema
        .type_by_name(&schema_root.type_name)
        .and_then(|ty| ty.field(&field.name.value))
    else {
        return Err(Box::new(invariant_response(
            &engine,
            "validated subscription field definition is missing",
            engine.field_locations(field),
            vec![group.response_key.into()],
            Some((&schema_root.type_name, &field.name.value)),
        )));
    };
    let path = vec![group.response_key.into()];
    let arguments = engine
        .coerce_arguments(definition, &field.arguments, &path)
        .map_err(|errors| preflight_response(&engine, errors))?;
    let directives = super::super::directive::coerce(&engine, group.directives.clone(), &path)
        .map_err(|errors| preflight_response(&engine, errors))?;
    let has_subselection = group
        .fields
        .iter()
        .any(|field| field.selection_set.is_some());
    Ok(PreparedSubscriptionPlan {
        root: PreparedSubscriptionRoot {
            operation: prepared.clone(),
            parent_type: schema_root.type_name.clone(),
            field_name: field.name.value.clone(),
            response_key: group.response_key.to_owned(),
            locations: engine.field_locations(field),
            arguments: arguments.public,
            directives,
            operation_directives,
            cost_fields,
        },
        dispatch_arguments: arguments.dispatch,
        has_subselection,
    })
}

fn preflight_response<C>(
    engine: &Engine<'_, C>,
    errors: Vec<crate::GraphQlError>,
) -> Box<GraphQlResponse> {
    Box::new(engine.output_byte_failure().unwrap_or(GraphQlResponse {
        data: None,
        errors,
        extensions: crate::Object::new(),
    }))
}

fn invariant_response<C>(
    engine: &Engine<'_, C>,
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

struct MetadataDispatch {
    runtime_byte_limit: Option<usize>,
}

impl FieldDispatch<()> for MetadataDispatch {
    fn object_scope(&self) -> Option<&ObjectScope> {
        None
    }

    fn runtime_byte_limit(&self) -> Option<usize> {
        self.runtime_byte_limit
    }

    fn resolve<'a>(
        &'a self,
        _context: &'a (),
        _parent_type: &'a str,
        _field_name: &'a str,
        _parent: Option<&'a ResolvedObject>,
        _arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async {
            Err(FieldError::internal(SubscriptionInvariant(
                "subscription preflight attempted resolver dispatch",
            )))
        })
    }
}

#[derive(Debug)]
struct SubscriptionInvariant(&'static str);

impl fmt::Display for SubscriptionInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for SubscriptionInvariant {}
