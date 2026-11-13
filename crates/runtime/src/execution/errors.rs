//! Client-safe execution errors and stable operation identities.

use chigiri_core::executable::OPERATION_NOT_SUPPORTED;
use chigiri_core::{
    DiagnosticBag, DocumentDigest, GraphQlError, GraphQlRequest, GraphQlResponse, Object,
    OperationIdentity, OperationKey, OperationName, Source,
};
use sha2::{Digest as _, Sha256};

pub(crate) fn lifecycle_result(response: &GraphQlResponse) -> chigiri_core::LifecycleResult {
    if response.data.is_none() && !response.errors.is_empty() {
        chigiri_core::LifecycleResult::Rejected
    } else {
        chigiri_core::LifecycleResult::Succeeded
    }
}

pub(crate) fn response_code(response: &GraphQlResponse) -> Option<&'static str> {
    match response
        .errors
        .first()
        .and_then(|error| error.extensions.get("code"))
        .and_then(chigiri_core::Value::as_str)
    {
        Some("GRAPHQL_PARSE_FAILED") => Some("GRAPHQL_PARSE_FAILED"),
        Some("GRAPHQL_VALIDATION_FAILED") => Some("GRAPHQL_VALIDATION_FAILED"),
        Some("OPERATION_NOT_FOUND") => Some("OPERATION_NOT_FOUND"),
        Some("BAD_USER_INPUT") => Some("BAD_USER_INPUT"),
        Some("METHOD_NOT_ALLOWED") => Some("METHOD_NOT_ALLOWED"),
        Some("RATE_LIMITED") => Some("RATE_LIMITED"),
        Some("OPERATION_COST_EXCEEDED") => Some("OPERATION_COST_EXCEEDED"),
        Some("INTERNAL_SERVER_ERROR") => Some("INTERNAL_SERVER_ERROR"),
        _ => None,
    }
}

pub(crate) fn operation_identity(
    schema_digest: chigiri_core::SchemaDigest,
    request: &GraphQlRequest,
    selected_name: Option<&str>,
) -> Result<OperationIdentity, Box<GraphQlResponse>> {
    let operation_name = selected_name
        .map(OperationName::new)
        .transpose()
        .map_err(|_| {
            Box::new(GraphQlResponse::request_error(
                GraphQlError::new("Operation name exceeds the supported limit")
                    .code("GRAPHQL_VALIDATION_FAILED"),
            ))
        })?;
    let digest = Sha256::digest(request.query.as_bytes());
    Ok(OperationIdentity::new(
        schema_digest,
        OperationKey::new(DocumentDigest::from_bytes(digest.into()), operation_name),
    ))
}

pub(crate) fn selection_error(
    error: chigiri_core::executable::OperationSelectionError,
) -> GraphQlResponse {
    let code = if error.code == OPERATION_NOT_SUPPORTED {
        "OPERATION_NOT_SUPPORTED"
    } else {
        "OPERATION_NOT_FOUND"
    };
    GraphQlResponse::request_error(GraphQlError::new(error.message).code(code))
}

pub(crate) fn diagnostics_response(
    diagnostics: DiagnosticBag,
    source: &Source,
    code: &'static str,
) -> GraphQlResponse {
    let mut errors = diagnostics
        .into_iter()
        .map(|diagnostic| {
            let mut error = GraphQlError::new(diagnostic.message).code(code);
            if let Some(location) = diagnostic.span.and_then(|span| source.location(span.start)) {
                error.locations.push(location);
            }
            error
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        errors.push(
            GraphQlError::new("Request failed after the diagnostic limit was reached").code(code),
        );
    }
    GraphQlResponse {
        data: None,
        errors,
        extensions: Object::new(),
    }
}
