//! Bounded transport metadata and request views available before body polling.

use std::error::Error;
use std::fmt;
use std::net::SocketAddr;

use crate::{RequestHeader, ResponseMediaType};
use chigiri_core::{DeadlineFactory, RequestTelemetry, TraceId};

const METADATA_FIELDS: usize = 16;
const METADATA_NAME_BYTES: usize = 64;
const METADATA_VALUE_BYTES: usize = 256;
const METADATA_TOTAL_BYTES: usize = 2_048;

/// A bounded metadata field explicitly supplied by the application.
#[derive(Clone, Eq, PartialEq)]
pub struct MetadataField {
    name: String,
    value: Vec<u8>,
}

impl MetadataField {
    /// Validates an ASCII name and bounded opaque value bytes.
    pub fn new(name: impl Into<String>, value: impl Into<Vec<u8>>) -> Result<Self, MetadataError> {
        let name = name.into();
        let value = value.into();
        if name.is_empty()
            || name.len() > METADATA_NAME_BYTES
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || value.len() > METADATA_VALUE_BYTES
        {
            return Err(MetadataError);
        }
        Ok(Self { name, value })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

impl fmt::Debug for MetadataField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MetadataField")
            .field("name", &self.name)
            .field("value_bytes", &self.value.len())
            .finish()
    }
}

/// A violation of application-metadata count, name, value, or total-byte limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataError;

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("transport metadata exceeds its bounded contract")
    }
}

impl Error for MetadataError {}

/// A bounded collection of application metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ApplicationMetadata(Vec<MetadataField>);

impl ApplicationMetadata {
    /// Validates field count and total bytes, then owns the metadata.
    pub fn new(fields: Vec<MetadataField>) -> Result<Self, MetadataError> {
        let total = fields.iter().try_fold(0usize, |total, field| {
            total.checked_add(field.name.len() + field.value.len())
        });
        if fields.len() > METADATA_FIELDS || total.is_none_or(|total| total > METADATA_TOTAL_BYTES)
        {
            return Err(MetadataError);
        }
        Ok(Self(fields))
    }

    /// Returns metadata fields in source order.
    pub fn fields(&self) -> &[MetadataField] {
        &self.0
    }
}

/// Borrowed URI view whose query value is redacted from `Debug` output.
#[derive(Clone, Copy)]
pub struct TransportUri<'a> {
    path: &'a str,
    query: Option<&'a str>,
}

impl<'a> TransportUri<'a> {
    pub const fn new(path: &'a str, query: Option<&'a str>) -> Self {
        Self { path, query }
    }
    pub const fn path(&self) -> &'a str {
        self.path
    }
    pub const fn query(&self) -> Option<&'a str> {
        self.query
    }
}

impl fmt::Debug for TransportUri<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportUri")
            .field("path", &self.path)
            .field("has_query", &self.query.is_some())
            .finish()
    }
}

/// Read-only request view passed to transport admission before body polling.
#[derive(Clone, Copy)]
pub struct TransportRequest<'a> {
    method: &'a str,
    uri: TransportUri<'a>,
    headers: &'a [RequestHeader<'a>],
    media: Option<ResponseMediaType>,
    peer_addr: Option<SocketAddr>,
    application_metadata: &'a ApplicationMetadata,
    trace_id: Option<&'a TraceId>,
    telemetry: Option<&'a RequestTelemetry>,
    instrumentation: Option<&'a chigiri_runtime::instrumentation::CapturedInstrumentation>,
    deadlines: Option<&'a dyn DeadlineFactory>,
}

impl<'a> TransportRequest<'a> {
    #[allow(clippy::too_many_arguments)]
    /// Creates a view from request parts validated by an adapter.
    pub const fn new(
        method: &'a str,
        path: &'a str,
        uri_query: Option<&'a str>,
        headers: &'a [RequestHeader<'a>],
        peer_addr: Option<SocketAddr>,
        application_metadata: &'a ApplicationMetadata,
    ) -> Self {
        Self {
            method,
            uri: TransportUri::new(path, uri_query),
            headers,
            media: None,
            peer_addr,
            application_metadata,
            trace_id: None,
            telemetry: None,
            instrumentation: None,
            deadlines: None,
        }
    }

    pub const fn method(&self) -> &'a str {
        self.method
    }
    pub const fn uri(&self) -> TransportUri<'a> {
        self.uri
    }
    pub const fn headers(&self) -> &'a [RequestHeader<'a>] {
        self.headers
    }
    /// Returns the response media type selected during endpoint preflight.
    pub const fn response_media_type(&self) -> Option<ResponseMediaType> {
        self.media
    }
    pub const fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }
    pub const fn application_metadata(&self) -> &'a ApplicationMetadata {
        self.application_metadata
    }

    /// Attaches an opaque trace ID extracted by the application or tracing backend.
    pub const fn with_trace_id(mut self, trace_id: &'a TraceId) -> Self {
        self.trace_id = Some(trace_id);
        self
    }

    pub const fn trace_id(&self) -> Option<&'a TraceId> {
        self.trace_id
    }

    /// Internal entry point through which the façade pipeline associates request identity.
    #[doc(hidden)]
    pub const fn __with_request_telemetry(mut self, telemetry: &'a RequestTelemetry) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub const fn request_telemetry(&self) -> Option<&'a RequestTelemetry> {
        self.telemetry
    }

    /// Forwards the immutable snapshot selected before an SDK task handoff.
    #[doc(hidden)]
    pub const fn __with_instrumentation_capture(
        mut self,
        captured: &'a chigiri_runtime::instrumentation::CapturedInstrumentation,
    ) -> Self {
        self.instrumentation = Some(captured);
        self
    }

    /// Returns an SDK-selected snapshot without reading a worker's ambient state.
    #[doc(hidden)]
    pub const fn __instrumentation_capture(
        &self,
    ) -> Option<&'a chigiri_runtime::instrumentation::CapturedInstrumentation> {
        self.instrumentation
    }

    /// Internal entry point through which the façade pipeline associates the
    /// same request timer.
    #[doc(hidden)]
    pub const fn __with_deadline_factory(mut self, deadlines: &'a dyn DeadlineFactory) -> Self {
        self.deadlines = Some(deadlines);
        self
    }

    /// Returns the runtime timer that the endpoint associated with the request.
    #[doc(hidden)]
    pub const fn deadline_factory(&self) -> Option<&'a dyn DeadlineFactory> {
        self.deadlines
    }

    /// Internal entry point through which the façade pipeline associates the
    /// negotiated media.
    #[doc(hidden)]
    pub const fn __with_response_media(mut self, media: ResponseMediaType) -> Self {
        self.media = Some(media);
        self
    }
}

impl fmt::Debug for TransportRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportRequest")
            .field("method", &self.method)
            .field("uri", &self.uri)
            .field("header_count", &self.headers.len())
            .field("media", &self.media)
            .field("has_peer_addr", &self.peer_addr.is_some())
            .field("metadata_count", &self.application_metadata.fields().len())
            .field("has_trace_id", &self.trace_id.is_some())
            .field("has_telemetry", &self.telemetry.is_some())
            .field("has_deadline_factory", &self.deadlines.is_some())
            .finish()
    }
}
