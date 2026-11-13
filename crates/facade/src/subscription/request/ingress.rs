//! Bounded transport capture before application coercion enters a worker.

use std::sync::Arc;

#[cfg(feature = "subscription-worker")]
use chigiri_http::RequestEnvelope;
use chigiri_http::{RequestMethod, ResponseMediaType, SubscriptionTransport, TransportRequest};
use chigiri_runtime::core::executor::SubscriptionMetadataCharge;
use chigiri_runtime::{GraphQlRequest, Object};

use super::{MetadataRetentionLimit, RetainedTransport, RetentionError};

pub(crate) struct SubscriberIngress {
    pub(super) transport: RetainedTransport,
    pub(super) method: RequestMethod,
    pub(super) media: ResponseMediaType,
    pub(super) profile: SubscriptionTransport,
    pub(super) authorized_init: Option<Arc<Object>>,
    pub(super) charged_bytes: usize,
}

/// Pre-spawn WebSocket metadata whose resolved document is not available yet.
#[cfg(feature = "subscription-worker")]
pub(crate) struct WebSocketSubscriberIngressSeed {
    transport: RetainedTransport,
    authorized_init: Option<Arc<Object>>,
    limit: MetadataRetentionLimit,
}

impl SubscriberIngress {
    pub(crate) const fn trace_id(&self) -> Option<&chigiri_runtime::TraceId> {
        self.transport.trace_id()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture(
        transport: TransportRequest<'_>,
        request: &GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
        profile: SubscriptionTransport,
        authorized_init: Option<Object>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        if profile != SubscriptionTransport::WebSocket && authorized_init.is_some() {
            return Err(RetentionError::InitializationForHttp);
        }
        Self::capture_inner(
            transport,
            request,
            method,
            media,
            profile,
            authorized_init.map(Arc::new),
            limit,
        )
    }

    /// Shares the connection-owned initialization value without copying its tree.
    #[cfg(all(test, feature = "subscription-worker"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn capture_websocket(
        transport: TransportRequest<'_>,
        request: &GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
        authorized_init: Option<Arc<Object>>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        Self::capture_inner(
            transport,
            request,
            method,
            media,
            SubscriptionTransport::WebSocket,
            authorized_init,
            limit,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn capture_inner(
        transport: TransportRequest<'_>,
        request: &GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
        profile: SubscriptionTransport,
        authorized_init: Option<Arc<Object>>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        let retained_init = authorized_init.as_ref();
        let transport = RetainedTransport::capture(
            transport,
            limit,
            |charge| match retained_init {
                Some(init) => charge.object(init.as_ref()),
                None => Some(()),
            },
            |charge| charge_request(charge, request),
        )?;
        let charged_bytes = transport.charged_bytes();
        Ok(Self {
            transport,
            method,
            media,
            profile,
            authorized_init,
            charged_bytes,
        })
    }
}

#[cfg(feature = "subscription-worker")]
impl WebSocketSubscriberIngressSeed {
    /// Captures every owned input and checks the normalized envelope lower bound.
    pub(crate) fn capture(
        transport: TransportRequest<'_>,
        envelope: &RequestEnvelope,
        authorized_init: Option<Arc<Object>>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        let retained_init = authorized_init.as_ref();
        let transport = RetainedTransport::capture(
            transport,
            limit,
            |charge| match retained_init {
                Some(init) => charge.object(init.as_ref()),
                None => Some(()),
            },
            |charge| charge_envelope(charge, envelope),
        )?;
        Ok(Self {
            transport,
            authorized_init,
            limit,
        })
    }

    /// Replaces the envelope lower bound with the exact resolved request charge.
    pub(crate) fn finish(
        self,
        request: &GraphQlRequest,
    ) -> Result<SubscriberIngress, RetentionError> {
        let mut charge = SubscriptionMetadataCharge::new();
        charge
            .add_bytes(self.transport.charged_bytes())
            .ok_or(RetentionError::Overflow)?;
        charge_request(&mut charge, request).ok_or(RetentionError::Overflow)?;
        self.limit.check(charge.bytes())?;
        Ok(SubscriberIngress {
            charged_bytes: self.transport.charged_bytes(),
            transport: self.transport,
            method: RequestMethod::Post,
            media: ResponseMediaType::GraphQlResponseJson,
            profile: SubscriptionTransport::WebSocket,
            authorized_init: self.authorized_init,
        })
    }
}

#[cfg(feature = "subscription-worker")]
fn charge_envelope(
    charge: &mut SubscriptionMetadataCharge,
    envelope: &RequestEnvelope,
) -> Option<()> {
    if let Some(document) = envelope.document_text() {
        charge.add_bytes(document.len())?;
    }
    if let Some(name) = envelope.operation_name() {
        charge.add_bytes(name.len())?;
    }
    charge.object(envelope.variables())?;
    charge.object(envelope.extensions())?;
    if let Some(reference) = envelope.persisted_reference() {
        charge.add_bytes(reference.id().len())?;
    }
    if let Some(telemetry) = envelope.request_telemetry() {
        if let Some(id) = telemetry.request_id() {
            charge.add_bytes(id.as_str().len())?;
        }
        if let Some(id) = telemetry
            .trace_id()
            .filter(|_| telemetry.propagation_context().is_none())
        {
            charge.add_bytes(id.as_str().len())?;
        }
    }
    Some(())
}

fn charge_request(charge: &mut SubscriptionMetadataCharge, request: &GraphQlRequest) -> Option<()> {
    charge.add_bytes(request.query.len())?;
    if let Some(name) = &request.operation_name {
        charge.add_bytes(name.len())?;
    }
    charge.object(&request.variables)?;
    charge.object(&request.extensions)?;
    if let Some(telemetry) = request.request_telemetry() {
        if let Some(id) = telemetry.request_id() {
            charge.add_bytes(id.as_str().len())?;
        }
        if let Some(id) = telemetry
            .trace_id()
            .filter(|_| telemetry.propagation_context().is_none())
        {
            charge.add_bytes(id.as_str().len())?;
        }
    }
    Some(())
}
