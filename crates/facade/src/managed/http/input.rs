//! Owned native HTTP metadata, before operation-specific subscription retention.

use std::net::SocketAddr;

use chigiri_http::{ApplicationMetadata, RequestHeader, TransportRequest};
use chigiri_runtime::instrumentation::CapturedInstrumentation;

pub(super) struct Input {
    method: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, Vec<u8>)>,
    metadata: ApplicationMetadata,
    peer: Option<SocketAddr>,
    trace: Option<chigiri_http::TraceId>,
    instrumentation: Option<CapturedInstrumentation>,
}

impl Input {
    pub(super) fn capture(
        request: TransportRequest<'_>,
        instrumentation: Option<CapturedInstrumentation>,
    ) -> Self {
        Self {
            method: request.method().to_owned(),
            path: request.uri().path().to_owned(),
            query: request.uri().query().map(str::to_owned),
            headers: request
                .headers()
                .iter()
                .map(|header| (header.name.to_owned(), header.value.to_vec()))
                .collect(),
            metadata: request.application_metadata().clone(),
            peer: request.peer_addr(),
            trace: request.trace_id().cloned(),
            instrumentation,
        }
    }

    pub(super) fn instrumentation(&self) -> Option<&CapturedInstrumentation> {
        self.instrumentation.as_ref()
    }

    pub(super) fn headers(&self) -> Vec<RequestHeader<'_>> {
        self.headers
            .iter()
            .map(|(name, value)| RequestHeader::new(name, value))
            .collect()
    }

    pub(super) fn view<'a>(&'a self, headers: &'a [RequestHeader<'a>]) -> TransportRequest<'a> {
        let mut request = TransportRequest::new(
            &self.method,
            &self.path,
            self.query.as_deref(),
            headers,
            self.peer,
            &self.metadata,
        );
        if let Some(trace) = &self.trace {
            request = request.with_trace_id(trace);
        }
        if let Some(captured) = &self.instrumentation {
            request = request.__with_instrumentation_capture(captured);
        }
        request
    }
}
