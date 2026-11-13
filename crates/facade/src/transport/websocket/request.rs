//! Bounded, credential-redacted ownership of an admitted opening request.

use std::fmt;

use chigiri_http::TransportRequest;
#[cfg(feature = "subscription-worker")]
use chigiri_http::{ConnectionInitAuthorizer, ConnectionInitDecision, ConnectionInitError};
#[cfg(feature = "subscription-worker")]
use chigiri_runtime::Object;
use chigiri_runtime::RequestTelemetry;
use chigiri_runtime::core::executor::SubscriptionMetadataCharge;
use chigiri_runtime::instrumentation::CapturedInstrumentation;

use crate::subscription::request::{MetadataRetentionLimit, RetainedTransport, RetentionError};

pub(crate) struct AdmittedWebSocketRequest {
    transport: RetainedTransport,
    telemetry: RequestTelemetry,
    key: String,
    instrumentation: Option<CapturedInstrumentation>,
}

impl AdmittedWebSocketRequest {
    pub(super) fn capture(
        transport: TransportRequest<'_>,
        telemetry: &RequestTelemetry,
        key: &str,
        limit: MetadataRetentionLimit,
        captured: Option<&CapturedInstrumentation>,
    ) -> Result<Self, RetentionError> {
        let (transport, (telemetry, instrumentation)) = RetainedTransport::capture_with_optional(
            transport,
            limit,
            |charge| charge_retained(charge, telemetry, key),
            |_| Some(()),
            |available| retain_trace_metadata(telemetry, captured, available),
        )?;
        Ok(Self {
            transport,
            telemetry,
            key: key.to_owned(),
            instrumentation,
        })
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn instrumentation(&self) -> Option<&CapturedInstrumentation> {
        self.instrumentation.as_ref()
    }

    pub(super) fn inspect_transport<R>(
        &self,
        inspect: impl FnOnce(TransportRequest<'_>) -> R,
    ) -> R {
        self.transport.inspect(Some(&self.telemetry), |request| {
            let request = match &self.instrumentation {
                Some(captured) => request.__with_instrumentation_capture(captured),
                None => request,
            };
            inspect(request)
        })
    }

    #[cfg(feature = "subscription-worker")]
    pub(super) async fn authorize(
        &self,
        authorizer: &dyn ConnectionInitAuthorizer,
        payload: Option<&Object>,
        activity: &crate::managed::EventActivity,
    ) -> Result<ConnectionInitDecision, ConnectionInitError> {
        self.transport
            .authorize_connection_init(&self.telemetry, payload, authorizer, activity)
            .await
    }

    pub(super) const fn telemetry(&self) -> &RequestTelemetry {
        &self.telemetry
    }

    pub(super) fn key(&self) -> &str {
        self.key.as_str()
    }

    pub(super) const fn charged_bytes(&self) -> usize {
        self.transport.charged_bytes()
    }
}

fn charge_retained(
    charge: &mut SubscriptionMetadataCharge,
    telemetry: &RequestTelemetry,
    key: &str,
) -> Option<()> {
    charge.add_bytes(key.len())?;
    if let Some(id) = telemetry.request_id() {
        charge.add_bytes(id.as_str().len())?;
    }
    if telemetry.propagation_context().is_none()
        && let Some(id) = telemetry.trace_id()
    {
        charge.add_bytes(id.as_str().len())?;
    }
    Some(())
}

fn retain_trace_metadata(
    telemetry: &RequestTelemetry,
    captured: Option<&CapturedInstrumentation>,
    available: usize,
) -> ((RequestTelemetry, Option<CapturedInstrumentation>), usize) {
    let configured = telemetry.propagation_context().is_some();
    let id_bytes = telemetry.trace_id().map_or(0, |id| id.as_str().len());
    // The opening retains its incoming context separately. Public correlation
    // keeps no duplicate request parent or live span and is optional when new.
    let telemetry = telemetry.retained_instrumentation(available.min(id_bytes));
    let id_bytes = if configured {
        telemetry.trace_id().map_or(0, |id| id.as_str().len())
    } else {
        0
    };
    let captured = captured.map(|captured| captured.retain_within(available - id_bytes));
    let bytes = id_bytes
        + captured
            .as_ref()
            .map_or(0, |captured| captured.values().correlation_bytes());
    ((telemetry, captured), bytes)
}

impl fmt::Debug for AdmittedWebSocketRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedWebSocketRequest")
            .field("charged_bytes", &self.charged_bytes())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use chigiri_http::{ApplicationMetadata, MetadataField, RequestHeader, TransportRequest};
    use chigiri_runtime::{RequestId, RequestTelemetry, SamplingDecision, TraceId};

    use super::*;

    #[test]
    fn exact_limit_counts_every_owned_copy_before_capture() {
        let metadata =
            ApplicationMetadata::new(vec![MetadataField::new("a", b"bc").unwrap()]).unwrap();
        let headers = [RequestHeader::new("private", b"credential")];
        let trace = TraceId::new("transport-trace").unwrap();
        let transport = TransportRequest::new(
            "GET",
            "/graphql",
            Some("secret=1"),
            &headers,
            None,
            &metadata,
        )
        .with_trace_id(&trace);
        let telemetry = RequestTelemetry::new(
            Some(RequestId::new("request-id").unwrap()),
            Some(TraceId::new("telemetry-trace").unwrap()),
            SamplingDecision::Record,
        );
        let expected = 3 + 8 + 8 + 16 + 7 + 10 + 16 + 1 + 2 + 15 + 24 + 10 + 15;
        let capture = |limit| {
            AdmittedWebSocketRequest::capture(
                transport,
                &telemetry,
                "dGhlIHNhbXBsZSBub25jZQ==",
                MetadataRetentionLimit::new(limit).unwrap(),
                None,
            )
        };
        assert_eq!(capture(expected).unwrap().charged_bytes(), expected);
        assert_eq!(capture(expected - 1).unwrap_err(), RetentionError::Exceeded);
    }

    #[test]
    fn diagnostics_never_disclose_owned_credentials() {
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("authorization", b"secret-credential")];
        let transport = TransportRequest::new(
            "GET",
            "/private-path",
            Some("token=private"),
            &headers,
            None,
            &metadata,
        );
        let request = AdmittedWebSocketRequest::capture(
            transport,
            &RequestTelemetry::default(),
            "dGhlIHNhbXBsZSBub25jZQ==",
            MetadataRetentionLimit::default(),
            None,
        )
        .unwrap();
        let debug = format!("{request:?}");
        for secret in [
            "secret-credential",
            "/private-path",
            "token=private",
            "dGhlIHNhbXBsZSBub25jZQ==",
        ] {
            assert!(!debug.contains(secret));
        }
    }
}
