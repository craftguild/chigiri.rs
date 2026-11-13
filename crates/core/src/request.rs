use std::fmt;

use crate::{Object, RequestTelemetry};

/// GraphQL request passed from transport to core execution.
#[derive(Clone, PartialEq)]
pub struct GraphQlRequest {
    /// GraphQL document source text.
    pub query: String,
    /// Optional operation name used to select among multiple operations.
    pub operation_name: Option<String>,
    /// Raw variable object.
    pub variables: Object,
    /// Request extensions passed to transports and plugins.
    pub extensions: Object,
    /// Verified catalog cost hint set by the operation source.
    #[doc(hidden)]
    pub catalog_cost_hint: Option<u64>,
    /// Correlation and sampling snapshot captured when the request started.
    #[doc(hidden)]
    pub telemetry: Option<RequestTelemetry>,
}

impl fmt::Debug for GraphQlRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphQlRequest")
            .field("query_bytes", &self.query.len())
            .field("has_operation_name", &self.operation_name.is_some())
            .field("variable_count", &self.variables.len())
            .field("extension_count", &self.extensions.len())
            .field("has_catalog_cost_hint", &self.catalog_cost_hint.is_some())
            .field("has_telemetry", &self.telemetry.is_some())
            .finish()
    }
}

impl GraphQlRequest {
    /// Creates a request with a document and empty optional fields.
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            operation_name: None,
            variables: Object::new(),
            extensions: Object::new(),
            catalog_cost_hint: None,
            telemetry: None,
        }
    }

    /// Sets the selected operation name.
    pub fn with_operation_name(mut self, operation_name: impl Into<String>) -> Self {
        self.operation_name = Some(operation_name.into());
        self
    }

    /// Sets raw variable values.
    pub fn with_variables(mut self, variables: Object) -> Self {
        self.variables = variables;
        self
    }

    /// Sets request extensions.
    pub fn with_extensions(mut self, extensions: Object) -> Self {
        self.extensions = extensions;
        self
    }

    /// Sets a positive catalog cost hint validated by the operation source.
    #[doc(hidden)]
    pub fn with_catalog_cost_hint(mut self, cost_hint: u64) -> Self {
        self.catalog_cost_hint = (cost_hint > 0).then_some(cost_hint);
        self
    }

    /// Returns the validated catalog cost hint supplied by source resolution.
    pub const fn catalog_cost_hint(&self) -> Option<u64> {
        self.catalog_cost_hint
    }

    /// Set the request telemetry created by endpoint/direct runtime.
    #[doc(hidden)]
    pub fn with_request_telemetry(mut self, telemetry: RequestTelemetry) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub const fn request_telemetry(&self) -> Option<&RequestTelemetry> {
        self.telemetry.as_ref()
    }
}
