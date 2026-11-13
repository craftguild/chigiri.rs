//! Shared parse, validation, and validated-document cache path.

use std::sync::Arc;

use chigiri_core::executable::validate_executable_document_with_scalars;
use chigiri_core::language::parse_executable_document;
use chigiri_core::{GraphQlRequest, GraphQlResponse, Source, SourceId};

use super::diagnostics_response;
use crate::ExecutableSchema;
use crate::diagnostics::request_identity;

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    pub(crate) fn validated_document(
        &self,
        request: &GraphQlRequest,
        source: &Source,
    ) -> Result<Arc<chigiri_core::executable::ValidatedDocument>, Box<GraphQlResponse>> {
        if let Some(document) = self.inner.cache.get(&request.query) {
            self.inner.diagnostics.observe(
                chigiri_core::LifecyclePhase::DocumentPreparation,
                chigiri_core::LifecycleResult::CacheHit,
                None,
                request_identity(None, request.request_telemetry()),
                None,
                "document-cache",
            );
            return Ok(document);
        }
        self.inner.diagnostics.observe(
            chigiri_core::LifecyclePhase::DocumentPreparation,
            chigiri_core::LifecycleResult::CacheMiss,
            None,
            request_identity(None, request.request_telemetry()),
            None,
            "document-cache",
        );
        let parsed =
            parse_executable_document(SourceId::new(0), &request.query, &self.inner.limits);
        let Some(document) = parsed.document else {
            return Err(Box::new(diagnostics_response(
                parsed.diagnostics,
                source,
                "GRAPHQL_PARSE_FAILED",
            )));
        };
        let validated = validate_executable_document_with_scalars(
            &self.inner.semantic_schema,
            &document,
            &self.inner.scalars,
            &self.inner.limits,
        );
        let Some(document) = validated.document else {
            return Err(Box::new(diagnostics_response(
                validated.diagnostics,
                source,
                "GRAPHQL_VALIDATION_FAILED",
            )));
        };
        Ok(self
            .inner
            .cache
            .insert(request.query.clone(), Arc::new(document)))
    }
}
