//! Request, operation, and field context exposed to middleware.

use crate::executable::{CoercedVariables, ComplexitySummary};
use crate::language::ast::OperationType;
use crate::{
    DeadlineFactory, GraphQlRequest, Object, OperationIdentity, PathSegment, RequestTelemetry,
};

use super::CostFieldMetadata;
use super::ExecutableDirective;

/// Raw request context available before parsing and cache lookup.
#[derive(Clone, Copy)]
pub struct RequestContext<'a, C> {
    context: &'a C,
    request: &'a GraphQlRequest,
}

impl<'a, C> RequestContext<'a, C> {
    /// Creates request context from the application context and raw request.
    pub const fn new(context: &'a C, request: &'a GraphQlRequest) -> Self {
        Self { context, request }
    }

    /// Returns the request-scoped application context.
    pub const fn context(&self) -> &'a C {
        self.context
    }

    /// Returns the GraphQL request before parsing.
    pub const fn request(&self) -> &'a GraphQlRequest {
        self.request
    }
}

/// Operation context available after validation, selection, and coercion.
#[derive(Clone, Copy)]
pub struct OperationContext<'a, C> {
    context: &'a C,
    operation_type: OperationType,
    operation_name: Option<&'a str>,
    variables: &'a CoercedVariables,
    complexity: ComplexitySummary,
    directives: &'a [ExecutableDirective],
    identity: Option<&'a OperationIdentity>,
    cost_fields: &'a [CostFieldMetadata],
    catalog_cost_hint: Option<u64>,
    telemetry: Option<&'a RequestTelemetry>,
    deadlines: Option<&'a dyn DeadlineFactory>,
}

impl<'a, C> OperationContext<'a, C> {
    /// Creates metadata for the selected operation independent of custom directives.
    pub const fn new(
        context: &'a C,
        operation_type: OperationType,
        operation_name: Option<&'a str>,
        variables: &'a CoercedVariables,
        complexity: ComplexitySummary,
    ) -> Self {
        Self::execution(
            context,
            operation_type,
            operation_name,
            variables,
            complexity,
            &[],
            None,
            &[],
            None,
            None,
            None,
        )
    }

    /// Create an execution context containing coerced operation metadata.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub const fn execution(
        context: &'a C,
        operation_type: OperationType,
        operation_name: Option<&'a str>,
        variables: &'a CoercedVariables,
        complexity: ComplexitySummary,
        directives: &'a [ExecutableDirective],
        identity: Option<&'a OperationIdentity>,
        cost_fields: &'a [CostFieldMetadata],
        catalog_cost_hint: Option<u64>,
        telemetry: Option<&'a RequestTelemetry>,
        deadlines: Option<&'a dyn DeadlineFactory>,
    ) -> Self {
        Self {
            context,
            operation_type,
            operation_name,
            variables,
            complexity,
            directives,
            identity,
            cost_fields,
            catalog_cost_hint,
            telemetry,
            deadlines,
        }
    }

    /// Returns the request-scoped application context.
    pub const fn context(&self) -> &'a C {
        self.context
    }

    /// Returns the selected operation.
    pub const fn operation_type(&self) -> OperationType {
        self.operation_type
    }

    /// Returns the explicit operation name, if any.
    pub const fn operation_name(&self) -> Option<&'a str> {
        self.operation_name
    }

    /// Returns the coerced variables for this request.
    pub const fn variables(&self) -> &'a CoercedVariables {
        self.variables
    }

    /// Returns operation metrics collected during validation.
    pub const fn complexity(&self) -> ComplexitySummary {
        self.complexity
    }

    /// Returns custom directives syntactically reachable from the selected
    /// operation in expansion order.
    ///
    /// Available to operation-wide policies for every field, including introspection meta-fields.
    pub const fn directives(&self) -> &'a [ExecutableDirective] {
        self.directives
    }

    /// Returns the identity calculated from schema/document/selection operation.
    pub const fn identity(&self) -> Option<&'a OperationIdentity> {
        self.identity
    }

    /// Returns the schema-coordinate and argument cost view after fragment expansion.
    pub const fn cost_fields(&self) -> &'a [CostFieldMetadata] {
        self.cost_fields
    }

    /// Returns the verified catalog cost hint from the operation source.
    pub const fn catalog_cost_hint(&self) -> Option<u64> {
        self.catalog_cost_hint
    }

    pub const fn request_telemetry(&self) -> Option<&'a RequestTelemetry> {
        self.telemetry
    }

    /// Returns the same runtime timer supplied by the request execution source.
    pub const fn deadline_factory(&self) -> Option<&'a dyn DeadlineFactory> {
        self.deadlines
    }
}

/// Selection metadata exposed to field plugins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionMetadata<'a> {
    /// Response-object key after the alias is applied.
    pub response_key: &'a str,
    /// Composite subselection containing one or more fields.
    pub has_subselection: bool,
}

/// The schema coordinate context shared by the resolver and field middleware.
#[derive(Clone, Copy)]
pub struct FieldContext<'a, C> {
    context: &'a C,
    parent_type: &'a str,
    field_name: &'a str,
    path: &'a [PathSegment],
    arguments: Option<&'a Object>,
    parent: Option<&'a super::ResolvedObject>,
    selection: Option<SelectionMetadata<'a>>,
    directives: &'a [ExecutableDirective],
    identity: Option<&'a OperationIdentity>,
    telemetry: Option<&'a RequestTelemetry>,
}

impl<'a, C> FieldContext<'a, C> {
    /// Creates the minimal context for a generated resolver.
    pub const fn new(context: &'a C, parent_type: &'a str, field_name: &'a str) -> Self {
        Self {
            context,
            parent_type,
            field_name,
            path: &[],
            arguments: None,
            parent: None,
            selection: None,
            directives: &[],
            identity: None,
            telemetry: None,
        }
    }

    /// Creates a field-middleware context with validated metadata.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub const fn execution(
        context: &'a C,
        parent_type: &'a str,
        field_name: &'a str,
        path: &'a [PathSegment],
        arguments: &'a Object,
        selection: SelectionMetadata<'a>,
        directives: &'a [ExecutableDirective],
        identity: Option<&'a OperationIdentity>,
        telemetry: Option<&'a RequestTelemetry>,
    ) -> Self {
        Self {
            context,
            parent_type,
            field_name,
            path,
            arguments: Some(arguments),
            parent: None,
            selection: Some(selection),
            directives,
            identity,
            telemetry,
        }
    }

    /// Associates the same parent value that the executor passes to the
    /// resolver; root fields retain `None`.
    #[doc(hidden)]
    pub const fn __with_parent(mut self, parent: Option<&'a super::ResolvedObject>) -> Self {
        self.parent = parent;
        self
    }

    /// Borrows the parent value passed to resolver dispatch without executing child selections.
    pub const fn parent(&self) -> Option<&'a super::ResolvedObject> {
        self.parent
    }

    /// It returns the request context of the application.
    pub const fn context(&self) -> &'a C {
        self.context
    }

    /// Returns the parent object name in the schema.
    pub const fn parent_type(&self) -> &'a str {
        self.parent_type
    }

    /// Returns the field name in the schema.
    pub const fn field_name(&self) -> &'a str {
        self.field_name
    }

    /// Returns the path from the response root to the current field.
    pub const fn path(&self) -> &'a [PathSegment] {
        self.path
    }

    /// Returns coerced arguments available to middleware.
    pub const fn arguments(&self) -> Option<&'a Object> {
        self.arguments
    }

    /// It returns the selection metadata when middleware is running.
    pub const fn selection(&self) -> Option<SelectionMetadata<'a>> {
        self.selection
    }

    /// Returns the fragment ancestry of the merged field occurrence and the direct application sequence.
    pub const fn directives(&self) -> &'a [ExecutableDirective] {
        self.directives
    }

    /// Returns the selected operation identity.
    pub const fn identity(&self) -> Option<&'a OperationIdentity> {
        self.identity
    }

    pub const fn request_telemetry(&self) -> Option<&'a RequestTelemetry> {
        self.telemetry
    }
}
