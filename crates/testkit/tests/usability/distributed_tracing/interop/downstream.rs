//! One bounded application-owned loopback HTTP server using released extraction.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use opentelemetry::Context;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::{Span, SpanContext, TraceContextExt, Tracer};
use opentelemetry_sdk::propagation::TraceContextPropagator;

use super::super::recording::CountedTracer;

#[derive(Debug)]
pub(super) struct Observed {
    pub(super) parent: SpanContext,
    pub(super) child: SpanContext,
}

struct Headers<'a>(&'a hyper::HeaderMap);
impl Extractor for Headers<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)?.to_str().ok()
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|key| key.as_str()).collect()
    }
}

pub(super) async fn start(
    tracer: CountedTracer,
) -> (SocketAddr, tokio::task::JoinHandle<Observed>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(2), async move {
            let (stream, _) = listener.accept().await.unwrap();
            let observed = Arc::new(Mutex::new(None));
            let output = Arc::clone(&observed);
            let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                let context = TraceContextPropagator::new()
                    .extract_with_context(&Context::new(), &Headers(request.headers()));
                let parent = context.span().span_context().clone();
                let mut span = tracer.start_with_context("application.downstream", &context);
                let child = span.span_context().clone();
                span.end();
                assert!(
                    output
                        .lock()
                        .unwrap()
                        .replace(Observed { parent, child })
                        .is_none()
                );
                async {
                    Ok::<_, Infallible>(hyper::Response::new(Full::new(Bytes::from_static(b"ok"))))
                }
            });
            hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
                .unwrap();
            Arc::try_unwrap(observed)
                .unwrap()
                .into_inner()
                .unwrap()
                .unwrap()
        })
        .await
        .unwrap()
    });
    (address, task)
}
