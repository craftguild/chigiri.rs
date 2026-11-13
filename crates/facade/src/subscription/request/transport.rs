//! Checked capture of the transport metadata retained across async boundaries.

use std::fmt;

#[cfg(feature = "subscription-worker")]
use chigiri_http::{
    ConnectionInitAuthorizer, ConnectionInitContext, ConnectionInitDecision, ConnectionInitError,
};
use chigiri_http::{RequestHeader, TransportRequest};
#[cfg(feature = "subscription-worker")]
use chigiri_runtime::Object;
use chigiri_runtime::core::executor::SubscriptionMetadataCharge;

use super::{MetadataRetentionLimit, RetentionError};

pub(crate) struct RetainedTransport {
    method_token: String,
    path: String,
    raw_query: Option<String>,
    headers: Vec<OwnedHeader>,
    metadata: chigiri_http::ApplicationMetadata,
    peer_addr: Option<std::net::SocketAddr>,
    trace_id: Option<chigiri_runtime::TraceId>,
    charged_bytes: usize,
}

struct OwnedHeader {
    name: String,
    value: Vec<u8>,
}

impl RetainedTransport {
    pub(crate) fn capture(
        transport: TransportRequest<'_>,
        limit: MetadataRetentionLimit,
        retained_extra: impl FnOnce(&mut SubscriptionMetadataCharge) -> Option<()>,
        lower_bound: impl FnOnce(&mut SubscriptionMetadataCharge) -> Option<()>,
    ) -> Result<Self, RetentionError> {
        Self::capture_with_optional(transport, limit, retained_extra, lower_bound, |_| ((), 0))
            .map(|(transport, _)| transport)
    }

    pub(crate) fn capture_with_optional<T>(
        transport: TransportRequest<'_>,
        limit: MetadataRetentionLimit,
        retained_extra: impl FnOnce(&mut SubscriptionMetadataCharge) -> Option<()>,
        lower_bound: impl FnOnce(&mut SubscriptionMetadataCharge) -> Option<()>,
        retain: impl FnOnce(usize) -> (T, usize),
    ) -> Result<(Self, T), RetentionError> {
        let mut charge = SubscriptionMetadataCharge::new();
        charge_transport(&mut charge, transport).ok_or(RetentionError::Overflow)?;
        retained_extra(&mut charge).ok_or(RetentionError::Overflow)?;
        let mut charged_bytes = charge.bytes();
        lower_bound(&mut charge).ok_or(RetentionError::Overflow)?;
        let available = limit.available(charge.bytes())?;
        // The owner checks each optional value against this remaining capacity
        // before copying it. Required metadata has already passed its limit.
        let (retained, additional) = retain(available);
        charge
            .add_bytes(additional)
            .ok_or(RetentionError::Overflow)?;
        limit.check(charge.bytes())?;
        charged_bytes = charged_bytes
            .checked_add(additional)
            .ok_or(RetentionError::Overflow)?;
        Ok((
            Self {
                method_token: transport.method().to_owned(),
                path: transport.uri().path().to_owned(),
                raw_query: transport.uri().query().map(str::to_owned),
                headers: transport
                    .headers()
                    .iter()
                    .map(|header| OwnedHeader {
                        name: header.name.to_owned(),
                        value: header.value.to_vec(),
                    })
                    .collect(),
                metadata: transport.application_metadata().clone(),
                peer_addr: transport.peer_addr(),
                trace_id: transport.trace_id().cloned(),
                charged_bytes,
            },
            retained,
        ))
    }

    pub(super) fn headers(&self) -> Vec<RequestHeader<'_>> {
        self.headers
            .iter()
            .map(|header| RequestHeader::new(&header.name, &header.value))
            .collect()
    }

    pub(crate) fn inspect<R>(
        &self,
        telemetry: Option<&chigiri_runtime::RequestTelemetry>,
        inspect: impl FnOnce(TransportRequest<'_>) -> R,
    ) -> R {
        let headers = self.headers();
        let mut transport = TransportRequest::new(
            &self.method_token,
            &self.path,
            self.raw_query.as_deref(),
            &headers,
            self.peer_addr,
            &self.metadata,
        );
        if let Some(trace) = &self.trace_id {
            transport = transport.with_trace_id(trace);
        }
        if let Some(telemetry) = telemetry {
            transport = transport.__with_request_telemetry(telemetry);
        }
        inspect(transport)
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) async fn authorize_connection_init(
        &self,
        telemetry: &chigiri_runtime::RequestTelemetry,
        payload: Option<&Object>,
        authorizer: &dyn ConnectionInitAuthorizer,
        activity: &crate::managed::EventActivity,
    ) -> Result<ConnectionInitDecision, ConnectionInitError> {
        let headers = self.headers();
        let request = self.request(&headers).__with_request_telemetry(telemetry);
        crate::subscription::worker::TaskOwnedFuture::new(
            authorizer.authorize(ConnectionInitContext::__new(request, payload)),
            Some(activity.clone()),
        )
        .await
    }

    pub(super) fn request<'a>(&'a self, headers: &'a [RequestHeader<'a>]) -> TransportRequest<'a> {
        let mut transport = TransportRequest::new(
            &self.method_token,
            &self.path,
            self.raw_query.as_deref(),
            headers,
            self.peer_addr,
            &self.metadata,
        );
        if let Some(trace) = &self.trace_id {
            transport = transport.with_trace_id(trace);
        }
        transport
    }

    pub(super) const fn trace_id(&self) -> Option<&chigiri_runtime::TraceId> {
        self.trace_id.as_ref()
    }

    pub(crate) const fn charged_bytes(&self) -> usize {
        self.charged_bytes
    }
}

impl fmt::Debug for RetainedTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetainedTransport")
            .field("header_count", &self.headers.len())
            .field("metadata_count", &self.metadata.fields().len())
            .field("charged_bytes", &self.charged_bytes)
            .finish_non_exhaustive()
    }
}

pub(super) fn charge_transport(
    charge: &mut SubscriptionMetadataCharge,
    transport: TransportRequest<'_>,
) -> Option<()> {
    charge.add_bytes(transport.method().len())?;
    charge.add_bytes(transport.uri().path().len())?;
    if let Some(query) = transport.uri().query() {
        charge.add_bytes(query.len())?;
    }
    charge.add_entries(transport.headers().len())?;
    for header in transport.headers() {
        charge.add_bytes(header.name.len())?;
        charge.add_bytes(header.value.len())?;
    }
    charge.add_entries(transport.application_metadata().fields().len())?;
    for field in transport.application_metadata().fields() {
        charge.add_bytes(field.name().len())?;
        charge.add_bytes(field.value().len())?;
    }
    if let Some(trace) = transport.trace_id() {
        charge.add_bytes(trace.as_str().len())?;
    }
    Some(())
}
