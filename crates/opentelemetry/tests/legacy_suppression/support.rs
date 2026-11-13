use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use chigiri_opentelemetry::OpenTelemetryObserver;
use chigiri_runtime::{
    LifecycleEvent, LifecycleObserver, LifecyclePhase, LifecycleResult, RequestId,
    RequestTelemetry, SamplingDecision,
};
use opentelemetry::trace::{Span, SpanBuilder, SpanContext, Status, Tracer, TracerProvider};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{
    Sampler, SdkTracer, SdkTracerProvider, Span as SdkSpan, SpanData, SpanProcessor,
};

#[derive(Clone, Debug, Default)]
pub struct Counts(Arc<Counters>);

#[derive(Debug, Default)]
struct Counters {
    starts: AtomicUsize,
    ends: AtomicUsize,
    drops: AtomicUsize,
}

impl Counts {
    pub fn assert(&self, starts: usize, ends: usize, drops: usize) {
        assert_eq!(self.0.starts.load(Ordering::SeqCst), starts, "span starts");
        assert_eq!(self.0.ends.load(Ordering::SeqCst), ends, "span ends");
        assert_eq!(self.0.drops.load(Ordering::SeqCst), drops, "span drops");
    }
}

impl SpanProcessor for Counts {
    fn on_start(&self, _: &mut SdkSpan, _: &Context) {
        self.0.starts.fetch_add(1, Ordering::SeqCst);
    }

    fn on_end(&self, _: SpanData) {
        self.0.ends.fetch_add(1, Ordering::SeqCst);
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        Ok(())
    }
}

pub struct CountingTracer {
    inner: SdkTracer,
    counts: Counts,
}

impl Tracer for CountingTracer {
    type Span = CountedSpan;

    fn build_with_context(&self, builder: SpanBuilder, parent: &Context) -> Self::Span {
        CountedSpan {
            inner: Some(self.inner.build_with_context(builder, parent)),
            counts: self.counts.clone(),
        }
    }
}

pub struct CountedSpan {
    inner: Option<SdkSpan>,
    counts: Counts,
}

impl Drop for CountedSpan {
    fn drop(&mut self) {
        // Count physical release only after the actual SDK span is dropped.
        drop(self.inner.take());
        self.counts.0.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl Span for CountedSpan {
    fn add_event_with_timestamp<T>(
        &mut self,
        name: T,
        timestamp: SystemTime,
        attributes: Vec<KeyValue>,
    ) where
        T: Into<Cow<'static, str>>,
    {
        self.inner
            .as_mut()
            .unwrap()
            .add_event_with_timestamp(name, timestamp, attributes);
    }

    fn span_context(&self) -> &SpanContext {
        self.inner.as_ref().unwrap().span_context()
    }

    fn is_recording(&self) -> bool {
        self.inner.as_ref().unwrap().is_recording()
    }

    fn set_attribute(&mut self, attribute: KeyValue) {
        self.inner.as_mut().unwrap().set_attribute(attribute);
    }

    fn set_status(&mut self, status: Status) {
        self.inner.as_mut().unwrap().set_status(status);
    }

    fn update_name<T>(&mut self, name: T)
    where
        T: Into<Cow<'static, str>>,
    {
        self.inner.as_mut().unwrap().update_name(name);
    }

    fn add_link(&mut self, context: SpanContext, attributes: Vec<KeyValue>) {
        self.inner.as_mut().unwrap().add_link(context, attributes);
    }

    fn end_with_timestamp(&mut self, timestamp: SystemTime) {
        self.inner.as_mut().unwrap().end_with_timestamp(timestamp);
    }
}

pub fn fixture() -> (
    SdkTracerProvider,
    OpenTelemetryObserver<CountingTracer>,
    Counts,
) {
    let counts = Counts::default();
    let provider = SdkTracerProvider::builder()
        .with_sampler(Sampler::AlwaysOn)
        .with_span_processor(counts.clone())
        .build();
    let observer = OpenTelemetryObserver::new(CountingTracer {
        inner: provider.tracer("chigiri-legacy-suppression-test"),
        counts: counts.clone(),
    });
    (provider, observer, counts)
}

pub fn telemetry(id: Option<&str>) -> RequestTelemetry {
    RequestTelemetry::new(
        id.map(|value| RequestId::new(value).unwrap()),
        None,
        SamplingDecision::Record,
    )
}

pub fn emit(
    observer: &OpenTelemetryObserver<CountingTracer>,
    telemetry: &RequestTelemetry,
    phase: LifecyclePhase,
    result: LifecycleResult,
    component: &'static str,
) {
    observer.observe(LifecycleEvent::new(
        phase,
        result,
        None,
        telemetry.identity(None),
        None,
        component,
    ));
}
