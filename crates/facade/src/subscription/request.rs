//! Owned subscriber metadata with checked charges before source initialization.

#[cfg(feature = "subscription-worker")]
mod budget;
mod ingress;
mod limit;
mod transport;

use std::fmt;
use std::sync::Arc;

use chigiri_http::{
    DeadlineFactory, RequestContext, RequestHeader, RequestPhase, ResponseMediaType,
    SubscriptionTransport,
};
use chigiri_runtime::__private::PreparedSubscription;
#[cfg(feature = "subscription-worker")]
use chigiri_runtime::__private::PreparedWebSocketOperation;
use chigiri_runtime::core::executor::SubscriptionMetadataCharge;
use chigiri_runtime::{GraphQlRequest, OperationIdentity, RequestTelemetry, TraceId};

pub(crate) use ingress::SubscriberIngress;
#[cfg(feature = "subscription-worker")]
pub(crate) use ingress::WebSocketSubscriberIngressSeed;
pub(crate) use limit::{MetadataRetentionLimit, RetentionError};
pub(crate) use transport::RetainedTransport;

pub(crate) struct SubscriberRequest {
    ingress: SubscriberIngress,
    request: Option<Arc<GraphQlRequest>>,
    identity: Arc<OperationIdentity>,
    charged_bytes: usize,
}

impl SubscriberRequest {
    pub(crate) fn budget_instrumentation<C>(
        ingress: &SubscriberIngress,
        prepared: &mut PreparedSubscription<C>,
        limit: MetadataRetentionLimit,
    ) -> Result<(), RetentionError> {
        let required = prepared
            .required_retained_metadata_bytes()
            .and_then(|bytes| bytes.checked_add(ingress.charged_bytes))
            .ok_or(RetentionError::Overflow)?;
        prepared
            .retain_instrumentation(limit.available(required)?)
            .ok_or(RetentionError::Overflow)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn budget_websocket_instrumentation<C: Send + Sync + 'static>(
        ingress: &SubscriberIngress,
        prepared: &mut PreparedWebSocketOperation<C>,
        limit: MetadataRetentionLimit,
    ) -> Result<(), RetentionError> {
        let required = prepared
            .required_retained_metadata_bytes()
            .and_then(|bytes| bytes.checked_add(ingress.charged_bytes))
            .ok_or(RetentionError::Overflow)?;
        prepared
            .retain_subscription_instrumentation(limit.available(required)?)
            .ok_or(RetentionError::Overflow)
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retain<C>(
        transport: TransportRequest<'_>,
        prepared: &PreparedSubscription<C>,
        method: RequestMethod,
        media: ResponseMediaType,
        profile: SubscriptionTransport,
        authorized_init: Option<Object>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        let ingress = SubscriberIngress::capture(
            transport,
            &prepared.retained_request(),
            method,
            media,
            profile,
            authorized_init,
            limit,
        )?;
        Self::from_ingress(ingress, prepared, limit)
    }

    pub(crate) fn from_ingress<C>(
        ingress: SubscriberIngress,
        prepared: &PreparedSubscription<C>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        Self::from_prepared_parts(
            ingress,
            prepared.retained_metadata_bytes(),
            prepared.retained_request(),
            prepared.retained_identity(),
            limit,
        )
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn from_websocket<C: Send + Sync + 'static>(
        ingress: SubscriberIngress,
        prepared: &PreparedWebSocketOperation<C>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        Self::from_prepared_parts(
            ingress,
            prepared.retained_metadata_bytes(),
            prepared.retained_request(),
            prepared.retained_identity(),
            limit,
        )
    }

    fn from_prepared_parts(
        ingress: SubscriberIngress,
        runtime_bytes: Option<usize>,
        request: Arc<GraphQlRequest>,
        identity: Arc<OperationIdentity>,
        limit: MetadataRetentionLimit,
    ) -> Result<Self, RetentionError> {
        let runtime_bytes = runtime_bytes.ok_or(RetentionError::Overflow)?;
        let mut charge = SubscriptionMetadataCharge::new();
        charge
            .add_bytes(runtime_bytes)
            .ok_or(RetentionError::Overflow)?;
        charge
            .add_bytes(ingress.charged_bytes)
            .ok_or(RetentionError::Overflow)?;
        limit.check(charge.bytes())?;
        Ok(Self {
            ingress,
            request: Some(request),
            identity,
            charged_bytes: charge.bytes(),
        })
    }

    pub(crate) fn headers(&self) -> Vec<RequestHeader<'_>> {
        self.ingress.transport.headers()
    }

    pub(crate) fn context<'a>(
        &'a self,
        headers: &'a [RequestHeader<'a>],
        telemetry: &'a RequestTelemetry,
        phase: RequestPhase,
        deadlines: &'a dyn DeadlineFactory,
    ) -> RequestContext<'a> {
        let transport = self
            .ingress
            .transport
            .request(headers)
            .__with_request_telemetry(telemetry)
            .__with_deadline_factory(deadlines)
            .__with_response_media(self.ingress.media);
        RequestContext::__new(
            transport,
            self.request(),
            self.ingress.method,
            self.ingress.media,
        )
        .__with_phase(phase, self.ingress.authorized_init.as_deref())
    }

    pub(crate) const fn transport(&self) -> SubscriptionTransport {
        self.ingress.profile
    }

    pub(crate) const fn media(&self) -> ResponseMediaType {
        self.ingress.media
    }

    pub(crate) const fn trace_id(&self) -> Option<&TraceId> {
        self.ingress.transport.trace_id()
    }

    pub(crate) fn identity(&self) -> &OperationIdentity {
        &self.identity
    }

    pub(crate) fn request(&self) -> &GraphQlRequest {
        self.request
            .as_deref()
            .expect("subscriber request is attached outside synchronous rebudgeting")
    }

    pub(crate) const fn charged_bytes(&self) -> usize {
        self.charged_bytes
    }
}

impl fmt::Debug for SubscriberRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriberRequest")
            .field("transport", &self.ingress.profile)
            .field("charged_bytes", &self.charged_bytes)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

#[cfg(test)]
use chigiri_http::{ApplicationMetadata, RequestMethod, TransportRequest};
#[cfg(test)]
use chigiri_runtime::Object;
#[cfg(test)]
use transport::charge_transport;
