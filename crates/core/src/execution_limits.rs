//! Consistent resource limits for untrusted GraphQL requests.

/// Resource limits applied to parsing, validation, and execution.
///
/// [`Self::production`] defaults to a 256 KiB document, parser nesting 128,
/// operation depth 32, 1,000 fields after fragment expansion, 100 aliases, 100
/// variables, 100 directives, 100 diagnostics, 64 concurrently polled fields
/// per selection set, 1,000 items in each GraphQL or custom-scalar list,
/// custom-scalar output depth 128, and 100,000 output nodes per request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionLimits {
    /// Maximum bytes in a UTF-8 GraphQL document.
    pub max_document_bytes: usize,
    /// Maximum syntax nesting entered by the recursive-descent parser.
    pub max_parser_nesting: usize,
    /// Maximum validated operation depth.
    pub max_operation_depth: usize,
    /// Maximum total fields after fragment expansion.
    pub max_expanded_fields: usize,
    /// Maximum aliases in an operation.
    pub max_aliases: usize,
    /// Maximum variable definitions in an operation.
    pub max_variables: usize,
    /// The maximum number of directive applications allowed in a document.
    pub max_directives: usize,
    /// Maximum diagnostics retained per compiler phase.
    pub max_diagnostics: usize,
    /// Maximum fields polled concurrently in one selection set.
    pub max_concurrent_fields: usize,
    /// Maximum items in each GraphQL list and each custom-scalar list.
    pub max_list_items_per_field: usize,
    /// Maximum nesting depth of a JSON value serialized by a custom scalar.
    pub max_custom_scalar_output_depth: usize,
    /// Maximum combined number of completed fields, GraphQL list items, and
    /// custom-scalar object-member and list-item values in one request.
    pub max_output_nodes: usize,
}

impl ExecutionLimits {
    /// Returns the secure production default values used by the server and direct execution.
    pub const fn production() -> Self {
        Self {
            max_document_bytes: 256 * 1024,
            max_parser_nesting: 128,
            max_operation_depth: 32,
            max_expanded_fields: 1_000,
            max_aliases: 100,
            max_variables: 100,
            max_directives: 100,
            max_diagnostics: 100,
            max_concurrent_fields: 64,
            max_list_items_per_field: 1_000,
            max_custom_scalar_output_depth: 128,
            max_output_nodes: 100_000,
        }
    }

    /// Returns an effectively unbounded configuration selected explicitly by
    /// tests and internal tooling.
    ///
    /// This is never the server default. Consumers that add counts use
    /// `count >= limit` to avoid overflow.
    pub const fn unbounded() -> Self {
        Self {
            max_document_bytes: usize::MAX,
            max_parser_nesting: usize::MAX,
            max_operation_depth: usize::MAX,
            max_expanded_fields: usize::MAX,
            max_aliases: usize::MAX,
            max_variables: usize::MAX,
            max_directives: usize::MAX,
            max_diagnostics: usize::MAX,
            max_concurrent_fields: usize::MAX,
            max_list_items_per_field: usize::MAX,
            max_custom_scalar_output_depth: usize::MAX,
            max_output_nodes: usize::MAX,
        }
    }
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self::production()
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionLimits;

    #[test]
    fn production_defaults_are_bounded() {
        let limits = ExecutionLimits::production();

        assert_eq!(limits.max_document_bytes, 256 * 1024);
        assert_eq!(limits.max_parser_nesting, 128);
        assert_eq!(limits.max_operation_depth, 32);
        assert_eq!(limits.max_expanded_fields, 1_000);
        assert_eq!(limits.max_aliases, 100);
        assert_eq!(limits.max_variables, 100);
        assert_eq!(limits.max_directives, 100);
        assert_eq!(limits.max_diagnostics, 100);
        assert_eq!(limits.max_concurrent_fields, 64);
        assert_eq!(limits.max_list_items_per_field, 1_000);
        assert_eq!(limits.max_custom_scalar_output_depth, 128);
        assert_eq!(limits.max_output_nodes, 100_000);
        assert_eq!(ExecutionLimits::default(), limits);
    }

    #[test]
    fn unbounded_limits_are_an_explicit_opt_in() {
        let limits = ExecutionLimits::unbounded();

        assert_eq!(limits.max_document_bytes, usize::MAX);
        assert_eq!(limits.max_parser_nesting, usize::MAX);
        assert_eq!(limits.max_operation_depth, usize::MAX);
        assert_eq!(limits.max_expanded_fields, usize::MAX);
        assert_eq!(limits.max_aliases, usize::MAX);
        assert_eq!(limits.max_variables, usize::MAX);
        assert_eq!(limits.max_directives, usize::MAX);
        assert_eq!(limits.max_diagnostics, usize::MAX);
        assert_eq!(limits.max_concurrent_fields, usize::MAX);
        assert_eq!(limits.max_list_items_per_field, usize::MAX);
        assert_eq!(limits.max_custom_scalar_output_depth, usize::MAX);
        assert_eq!(limits.max_output_nodes, usize::MAX);
    }
}
