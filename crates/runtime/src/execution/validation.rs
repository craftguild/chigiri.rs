//! Offline operation validation with catalog and structural metadata.

use chigiri_core::{GraphQlRequest, GraphQlResponse, OperationIdentity, Source};

use super::{operation_identity, selection_error};
use crate::{ExecutableSchema, ValidatedOperation};

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    /// Validates a document against the schema and returns the selected operation identity.
    pub fn validate_operation(
        &self,
        request: &GraphQlRequest,
    ) -> Result<OperationIdentity, Box<GraphQlResponse>> {
        self.validate_operation_metadata(request)
            .map(|metadata| metadata.identity().clone())
    }

    /// Returns identity, structure, and result shape for offline catalog tooling.
    pub fn validate_operation_metadata(
        &self,
        request: &GraphQlRequest,
    ) -> Result<ValidatedOperation, Box<GraphQlResponse>> {
        let source = Source::new("<operation-catalog>", request.query.clone());
        let document = self
            .validated_document(request, &source)
            .map_err(|response| *response)?;
        let selected = document
            .select_operation(request.operation_name.as_deref())
            .map_err(selection_error)
            .map_err(Box::new)?;
        let identity = operation_identity(
            self.inner.info.schema_digest(),
            request,
            selected
                .operation
                .name
                .as_ref()
                .map(|name| name.value.as_str()),
        )?;
        ValidatedOperation::new(
            identity,
            &self.inner.semantic_schema,
            &document,
            selected,
            self.inner.resolvers.runtime_byte_limit(),
        )
        .map_err(|_| {
            Box::new(GraphQlResponse {
                data: None,
                errors: vec![
                    chigiri_core::GraphQlError::new("Operation metadata resource limit")
                        .code("BAD_USER_INPUT"),
                ],
                extensions: chigiri_core::Object::new(),
            })
        })
    }
}
