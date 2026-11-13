//! Connects persisted-operation modes to HTTP source resolution.

use std::sync::Arc;

use chigiri_http::{
    AdmissionError, BoxFuture, OperationSourceResolver, PersistedOperationProtocol, Rejection,
    RequestEnvelope, ResolvedOperationSource, SourceResolutionError,
};
use chigiri_runtime::{ErrorEvent, ErrorObserver, GraphQlError, LifecyclePhase, RequestIdentity};

use crate::digest::document_digest;
use crate::{
    ApqStore, OperationAuditEvent, OperationAuditOutcome, OperationAuditSink, OperationRegistry,
    OperationSourcePolicy, PersistedOperationId, RegistryDocument,
};

/// Resolves operation sources according to a fixed persisted-operation policy.
pub struct PersistedOperationResolver {
    policy: OperationSourcePolicy,
    registry: Option<Arc<dyn OperationRegistry>>,
    apq: Option<Arc<dyn ApqStore>>,
    audit_sink: Option<Arc<dyn OperationAuditSink>>,
    error_observer: Option<Arc<dyn ErrorObserver>>,
}

impl PersistedOperationResolver {
    /// Creates a normal query mode that rejects persisted-operation protocols.
    pub const fn disabled() -> Self {
        Self {
            policy: OperationSourcePolicy::Disabled,
            registry: None,
            apq: None,
            audit_sink: None,
            error_observer: None,
        }
    }

    /// Creates a writable Apollo APQ mode backed by `store`.
    pub fn apq(store: impl ApqStore) -> Self {
        let store = Arc::new(store);
        Self {
            policy: OperationSourcePolicy::Apq,
            registry: None,
            apq: Some(store),
            audit_sink: None,
            error_observer: None,
        }
    }

    /// Creates audit mode, which permits unknown operations and records their status.
    pub fn audit(
        registry: impl OperationRegistry,
        sink: impl OperationAuditSink,
        error_observer: impl ErrorObserver,
    ) -> Self {
        Self {
            policy: OperationSourcePolicy::Audit,
            registry: Some(Arc::new(registry)),
            apq: None,
            audit_sink: Some(Arc::new(sink)),
            error_observer: Some(Arc::new(error_observer)),
        }
    }

    /// Creates a safelist mode that accepts only catalogued operations.
    pub fn safelist(registry: impl OperationRegistry) -> Self {
        Self::registry(OperationSourcePolicy::Safelist, registry)
    }

    /// Creates a registry-only mode that rejects transport-supplied documents.
    pub fn id_only(registry: impl OperationRegistry) -> Self {
        Self::registry(OperationSourcePolicy::IdOnly, registry)
    }

    fn registry(policy: OperationSourcePolicy, registry: impl OperationRegistry) -> Self {
        Self {
            policy,
            registry: Some(Arc::new(registry)),
            apq: None,
            audit_sink: None,
            error_observer: None,
        }
    }

    async fn resolve_envelope(
        &self,
        envelope: &RequestEnvelope,
    ) -> Result<ResolvedOperationSource, SourceResolutionError> {
        if let (Some(document), Some(reference)) =
            (envelope.document_text(), envelope.persisted_reference())
            && reference.protocol() == PersistedOperationProtocol::ApolloApqV1
            && document_digest(document).to_hex() != reference.id()
        {
            return Err(reject(
                400,
                "Persisted operation hash mismatch",
                "PERSISTED_OPERATION_HASH_MISMATCH",
            ));
        }
        match self.policy {
            OperationSourcePolicy::Disabled => disabled(envelope),
            OperationSourcePolicy::Apq => self.resolve_apq(envelope).await,
            OperationSourcePolicy::Audit => self.resolve_audit(envelope).await,
            OperationSourcePolicy::Safelist => self.resolve_safelist(envelope).await,
            OperationSourcePolicy::IdOnly => self.resolve_id_only(envelope).await,
        }
    }

    async fn resolve_apq(
        &self,
        envelope: &RequestEnvelope,
    ) -> Result<ResolvedOperationSource, SourceResolutionError> {
        let Some(reference) = envelope.persisted_reference() else {
            return document_required(envelope).map(ResolvedOperationSource::new);
        };
        let id = persisted_id(reference.id())?;
        let store = self.apq.as_ref().expect("APQ mode keeps the store.");
        match lookup(store.as_ref(), &id).await? {
            Some(known) => {
                if envelope
                    .document_text()
                    .is_some_and(|document| document != known.document())
                {
                    return Err(hash_mismatch());
                }
                Ok(resolved_registry_document(known))
            }
            None => match envelope.document_text() {
                Some(document) => {
                    store
                        .put(id, Arc::<str>::from(document))
                        .await
                        .map_err(provider_failure)?;
                    Ok(ResolvedOperationSource::new(document))
                }
                None => Err(SourceResolutionError::Protocol(Box::new(
                    GraphQlError::new("Persisted operation was not found")
                        .code("PERSISTED_OPERATION_NOT_FOUND"),
                ))),
            },
        }
    }

    async fn resolve_audit(
        &self,
        envelope: &RequestEnvelope,
    ) -> Result<ResolvedOperationSource, SourceResolutionError> {
        let document = document_required(envelope)?;
        let digest = document_digest(&document);
        let identity = envelope.request_telemetry().map_or_else(
            || RequestIdentity::new(None, None, None),
            |telemetry| telemetry.identity(None),
        );
        let registry = self
            .registry
            .as_ref()
            .expect("Audit mode keeps the registry.");
        let lookup = match envelope.persisted_reference() {
            Some(reference) => match PersistedOperationId::new(reference.id()) {
                Ok(id) => registry.lookup(&id).await,
                Err(_) => Ok(None),
            },
            None => registry.lookup_by_document_digest(&digest).await,
        };
        let outcome = match lookup {
            Ok(Some(known)) if known.document() == document => OperationAuditOutcome::Hit,
            Ok(_) => OperationAuditOutcome::Miss,
            Err(error) => {
                self.observe_audit_error(
                    "OPERATION_AUDIT_PROVIDER_FAILED",
                    &error,
                    "operation-registry",
                    identity,
                );
                OperationAuditOutcome::ProviderUnavailable
            }
        };
        let event = OperationAuditEvent::new(
            identity,
            digest,
            registry.revision(),
            OperationSourcePolicy::Audit,
            outcome,
        );
        let sink = self
            .audit_sink
            .as_ref()
            .expect("Audit mode keeps the sink.");
        if let Err(error) = sink.record(event).await {
            self.observe_audit_error(
                "OPERATION_AUDIT_SINK_FAILED",
                &error,
                "operation-audit-sink",
                identity,
            );
        }
        Ok(ResolvedOperationSource::new(document))
    }

    async fn resolve_safelist(
        &self,
        envelope: &RequestEnvelope,
    ) -> Result<ResolvedOperationSource, SourceResolutionError> {
        let registry = self
            .registry
            .as_ref()
            .expect("Safelist mode keeps the registry.");
        let known = match envelope.persisted_reference() {
            Some(reference) => lookup(registry.as_ref(), &persisted_id(reference.id())?).await?,
            None => {
                let document = envelope.document_text().ok_or_else(not_allowed)?;
                registry
                    .lookup_by_document_digest(&document_digest(document))
                    .await
                    .map_err(provider_failure)?
            }
        };
        let Some(known) = known else {
            return Err(not_allowed());
        };
        if envelope
            .document_text()
            .is_some_and(|document| document != known.document())
        {
            return Err(not_allowed());
        }
        Ok(resolved_registry_document(known))
    }

    async fn resolve_id_only(
        &self,
        envelope: &RequestEnvelope,
    ) -> Result<ResolvedOperationSource, SourceResolutionError> {
        if envelope.document_text().is_some() {
            return Err(not_allowed());
        }
        let reference = envelope.persisted_reference().ok_or_else(not_allowed)?;
        let id = persisted_id(reference.id())?;
        let registry = self
            .registry
            .as_ref()
            .expect("IdOnly mode keeps the registry.");
        lookup(registry.as_ref(), &id)
            .await?
            .map(resolved_registry_document)
            .ok_or_else(not_allowed)
    }

    fn observe_audit_error(
        &self,
        code: &'static str,
        source: &(dyn std::error::Error + Send + Sync + 'static),
        component: &'static str,
        identity: RequestIdentity<'_>,
    ) {
        let Some(observer) = &self.error_observer else {
            return;
        };
        observer.observe_error(ErrorEvent::new(
            None,
            identity,
            LifecyclePhase::OperationSourceResolution,
            code,
            source,
            None,
            None,
            component,
        ));
    }
}

impl OperationSourceResolver for PersistedOperationResolver {
    fn resolve<'a>(
        &'a self,
        envelope: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        Box::pin(self.resolve_envelope(envelope))
    }
}

fn disabled(envelope: &RequestEnvelope) -> Result<ResolvedOperationSource, SourceResolutionError> {
    if envelope.persisted_reference().is_some() {
        return Err(reject(
            400,
            "Persisted operations are not supported",
            "PERSISTED_OPERATION_NOT_SUPPORTED",
        ));
    }
    document_required(envelope).map(ResolvedOperationSource::new)
}

fn document_required(envelope: &RequestEnvelope) -> Result<String, SourceResolutionError> {
    envelope
        .document_text()
        .map(str::to_owned)
        .ok_or_else(|| reject(400, "A GraphQL document is required", "BAD_REQUEST"))
}

async fn lookup<R>(
    registry: &R,
    id: &PersistedOperationId,
) -> Result<Option<RegistryDocument>, SourceResolutionError>
where
    R: OperationRegistry + ?Sized,
{
    registry.lookup(id).await.map_err(provider_failure)
}

fn persisted_id(value: &str) -> Result<PersistedOperationId, SourceResolutionError> {
    PersistedOperationId::new(value).map_err(|_| not_allowed())
}

fn hash_mismatch() -> SourceResolutionError {
    reject(
        400,
        "Persisted operation hash mismatch",
        "PERSISTED_OPERATION_HASH_MISMATCH",
    )
}

fn not_allowed() -> SourceResolutionError {
    reject(
        403,
        "Persisted operation is not allowed",
        "PERSISTED_OPERATION_NOT_ALLOWED",
    )
}

fn provider_failure(error: crate::ProviderError) -> SourceResolutionError {
    SourceResolutionError::Unavailable(AdmissionError::new(error))
}

fn resolved_registry_document(document: RegistryDocument) -> ResolvedOperationSource {
    let cost_hint = document.cost_hint();
    let source = ResolvedOperationSource::new(document.into_document().as_ref());
    match cost_hint {
        Some(cost) => source.with_catalog_cost_hint(cost),
        None => source,
    }
}

fn reject(status: u16, message: &'static str, code: &'static str) -> SourceResolutionError {
    let rejection = Rejection::new(status, GraphQlError::new(message).code(code))
        .expect("The fixed-source status is 400 to 599.");
    SourceResolutionError::Reject(Box::new(rejection))
}
