//! GraphQL-over-HTTP envelopes before and after persisted-source resolution.

use std::fmt;

use chigiri_core::{GraphQlRequest, Object, RequestTelemetry};

const APQ_HASH_BYTES: usize = 64;
const OPAQUE_ID_BYTES: usize = 256;

/// Persisted-operation protocols recognized by the built-in HTTP extractor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistedOperationProtocol {
    /// Apollo automatic persisted query version 1.
    ApolloApqV1,
    /// The application registry interprets the opaque persisted ID.
    Opaque,
}

/// A validated persisted-operation reference awaiting source resolution.
#[derive(Clone, Eq, PartialEq)]
pub struct PersistedOperationReference {
    protocol: PersistedOperationProtocol,
    id: String,
}

impl PersistedOperationReference {
    /// Creates an Apollo APQ reference from a 64-digit lowercase SHA-256 digest.
    pub fn apollo_apq_v1(sha256_hash: impl Into<String>) -> Result<Self, EnvelopeError> {
        let id = sha256_hash.into();
        if id.len() != APQ_HASH_BYTES
            || !id
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(EnvelopeError);
        }
        Ok(Self {
            protocol: PersistedOperationProtocol::ApolloApqV1,
            id,
        })
    }

    /// Creates an opaque reference from 1 to 256 visible ASCII bytes.
    pub fn opaque(id: impl Into<String>) -> Result<Self, EnvelopeError> {
        let id = id.into();
        if id.is_empty()
            || id.len() > OPAQUE_ID_BYTES
            || !id.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
        {
            return Err(EnvelopeError);
        }
        Ok(Self {
            protocol: PersistedOperationProtocol::Opaque,
            id,
        })
    }

    /// Returns the persisted-operation protocol.
    pub const fn protocol(&self) -> PersistedOperationProtocol {
        self.protocol
    }

    /// Returns the opaque lookup ID.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for PersistedOperationReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PersistedOperationReference")
            .field("protocol", &self.protocol)
            .field("id_bytes", &self.id.len())
            .finish()
    }
}

/// Invalid persisted-reference format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvelopeError;

impl fmt::Display for EnvelopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("persisted operation reference is malformed")
    }
}

impl std::error::Error for EnvelopeError {}

/// A GraphQL HTTP envelope that may or may not contain document text.
#[derive(Clone, PartialEq)]
pub struct RequestEnvelope {
    document: Option<String>,
    operation_name: Option<String>,
    variables: Object,
    extensions: Object,
    persisted: Option<PersistedOperationReference>,
    telemetry: Option<RequestTelemetry>,
}

impl RequestEnvelope {
    /// Wraps a normal GraphQL request whose source is already present.
    pub fn document(request: GraphQlRequest) -> Self {
        Self {
            document: Some(request.query),
            operation_name: request.operation_name,
            variables: request.variables,
            extensions: request.extensions,
            persisted: None,
            telemetry: request.telemetry,
        }
    }

    /// Creates an envelope from optional document text and a persisted reference.
    pub fn persisted(
        document: Option<String>,
        operation_name: Option<String>,
        variables: Object,
        extensions: Object,
        persisted: PersistedOperationReference,
    ) -> Self {
        Self {
            document,
            operation_name,
            variables,
            extensions,
            persisted: Some(persisted),
            telemetry: None,
        }
    }

    pub fn document_text(&self) -> Option<&str> {
        self.document.as_deref()
    }
    pub fn operation_name(&self) -> Option<&str> {
        self.operation_name.as_deref()
    }
    pub const fn variables(&self) -> &Object {
        &self.variables
    }
    pub const fn extensions(&self) -> &Object {
        &self.extensions
    }
    pub const fn persisted_reference(&self) -> Option<&PersistedOperationReference> {
        self.persisted.as_ref()
    }

    pub fn with_request_telemetry(mut self, telemetry: RequestTelemetry) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub const fn request_telemetry(&self) -> Option<&RequestTelemetry> {
        self.telemetry.as_ref()
    }

    /// Returns the core request after document-source resolution.
    pub fn into_document_request(self) -> Option<GraphQlRequest> {
        self.document.clone().map(|document| self.resolve(document))
    }

    /// Builds a core request with the exact document selected by the source resolver.
    pub fn resolve(self, document: String) -> GraphQlRequest {
        GraphQlRequest {
            query: document,
            operation_name: self.operation_name,
            variables: self.variables,
            extensions: self.extensions,
            catalog_cost_hint: None,
            telemetry: self.telemetry,
        }
    }
}

impl fmt::Debug for RequestEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestEnvelope")
            .field("has_document", &self.document.is_some())
            .field("has_operation_name", &self.operation_name.is_some())
            .field("variable_count", &self.variables.len())
            .field("extension_count", &self.extensions.len())
            .field("persisted", &self.persisted)
            .field("has_telemetry", &self.telemetry.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{PersistedOperationProtocol, PersistedOperationReference, RequestEnvelope};
    use chigiri_core::GraphQlRequest;

    #[test]
    fn debug_does_not_display_documents_and_variables() {
        let request = RequestEnvelope::document(GraphQlRequest::new("query Secret { value }"));
        assert!(!format!("{request:?}").contains("Secret"));
    }

    #[test]
    fn apq_hash_requires_lowercase_hex() {
        assert!(PersistedOperationReference::apollo_apq_v1("a".repeat(64)).is_ok());
        assert!(PersistedOperationReference::apollo_apq_v1("A".repeat(64)).is_err());
    }

    #[test]
    fn opaque_ids_accept_only_bounded_visible_ascii() {
        let reference = PersistedOperationReference::opaque("books-v1").unwrap();
        assert_eq!(reference.protocol(), PersistedOperationProtocol::Opaque);
        assert!(PersistedOperationReference::opaque("bad id").is_err());
        assert!(PersistedOperationReference::opaque("x".repeat(257)).is_err());
    }
}
