use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chigiri_runtime::instrumentation::{PropagationCarrier, PropagationField};
use opentelemetry::Context;
use opentelemetry::trace::{
    SpanBuilder, SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState, Tracer,
    TracerProvider,
};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{
    Sampler, SdkTracer, SdkTracerProvider, Span, SpanData, SpanProcessor,
};

use super::super::OpenTelemetryInstrumentation;

pub(super) const PARENT: &str = "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01";

pub(super) struct Headers(pub Vec<(PropagationField, Vec<u8>)>);

impl Headers {
    pub(super) fn parent() -> Self {
        Self(vec![(
            PropagationField::TraceParent,
            PARENT.as_bytes().to_vec(),
        )])
    }
}

impl PropagationCarrier for Headers {
    fn values(&self, field: PropagationField, visitor: &mut dyn FnMut(&[u8])) {
        for (key, value) in &self.0 {
            if *key == field {
                visitor(value);
            }
        }
    }
}

pub(super) struct MustNotRead;

impl PropagationCarrier for MustNotRead {
    fn values(&self, _: PropagationField, _: &mut dyn FnMut(&[u8])) {
        panic!("valid owning context must win without reading fallback headers");
    }
}

#[derive(Clone)]
pub(super) struct CountingTracer {
    inner: SdkTracer,
    pub(super) calls: Arc<AtomicUsize>,
}

impl Tracer for CountingTracer {
    type Span = Span;

    fn build_with_context(&self, builder: SpanBuilder, parent: &Context) -> Self::Span {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.build_with_context(builder, parent)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Records(Arc<Mutex<Vec<SpanData>>>);

impl Records {
    pub(super) fn take(&self) -> Vec<SpanData> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

impl SpanProcessor for Records {
    fn on_start(&self, _: &mut Span, _: &Context) {}

    fn on_end(&self, span: SpanData) {
        self.0.lock().unwrap().push(span);
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        Ok(())
    }
}

pub(super) fn provider() -> (
    SdkTracerProvider,
    OpenTelemetryInstrumentation<CountingTracer>,
    Records,
) {
    let records = Records::default();
    let provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::AlwaysOn)))
        .with_span_processor(records.clone())
        .build();
    let backend = OpenTelemetryInstrumentation::new(CountingTracer {
        inner: provider.tracer("chigiri-adr-0056-test"),
        calls: Arc::new(AtomicUsize::new(0)),
    });
    (provider, backend, records)
}

pub(super) fn parent(byte: u8, sampled: bool) -> Context {
    Context::new().with_remote_span_context(SpanContext::new(
        TraceId::from_bytes([byte; 16]),
        SpanId::from_bytes([byte; 8]),
        if sampled {
            TraceFlags::SAMPLED
        } else {
            TraceFlags::default()
        },
        true,
        TraceState::default(),
    ))
}
