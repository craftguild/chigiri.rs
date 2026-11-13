//! Application-owned released SDK with bounded test-only in-memory recording.

#[path = "recording/span.rs"]
mod lifetime;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opentelemetry::Context;
use opentelemetry::trace::{SpanBuilder, Tracer, TracerProvider};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{
    Sampler, SdkTracer, SdkTracerProvider, Span, SpanData, SpanProcessor,
};

#[derive(Clone)]
pub(super) struct CountedTracer {
    tracer: SdkTracer,
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) dropped: Arc<AtomicUsize>,
}

impl Tracer for CountedTracer {
    type Span = lifetime::CountedSpan;

    fn build_with_context(&self, builder: SpanBuilder, parent: &Context) -> Self::Span {
        self.calls.fetch_add(1, Ordering::SeqCst);
        lifetime::CountedSpan::new(
            self.tracer.build_with_context(builder, parent),
            Arc::clone(&self.dropped),
        )
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
        let mut records = self.0.lock().unwrap();
        assert!(records.len() < 128, "bounded recording fixture");
        records.push(span);
    }
    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }
    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        Ok(())
    }
}

pub(super) fn provider() -> (SdkTracerProvider, CountedTracer, Records) {
    let records = Records::default();
    let provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::AlwaysOn)))
        .with_span_processor(records.clone())
        .build();
    let tracer = CountedTracer {
        tracer: provider.tracer("chigiri-public-direct-consumer"),
        calls: Arc::new(AtomicUsize::new(0)),
        dropped: Arc::new(AtomicUsize::new(0)),
    };
    (provider, tracer, records)
}
