//! A record tracer to test bridge span parent relationships.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use opentelemetry::trace::{
    Span, SpanBuilder, SpanContext, SpanId, Status, TraceContextExt, TraceFlags, TraceId,
    TraceState, Tracer,
};
use opentelemetry::{Context, KeyValue};

#[derive(Clone, Debug)]
pub(crate) struct SpanRecord {
    pub(crate) name: String,
    pub(crate) id: SpanId,
    pub(crate) parent: SpanId,
}

#[derive(Clone, Default)]
pub(crate) struct RecordingTracer {
    next: Arc<AtomicU64>,
    records: Arc<Mutex<Vec<SpanRecord>>>,
}

impl RecordingTracer {
    pub(crate) fn records(&self) -> Vec<SpanRecord> {
        self.records.lock().unwrap().clone()
    }
}

impl Tracer for RecordingTracer {
    type Span = RecordingSpan;

    fn build_with_context(&self, builder: SpanBuilder, parent: &Context) -> Self::Span {
        let value = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let id = SpanId::from(value);
        self.records.lock().unwrap().push(SpanRecord {
            name: builder.name.into_owned(),
            id,
            parent: parent.span().span_context().span_id(),
        });
        RecordingSpan(SpanContext::new(
            TraceId::from(1),
            id,
            TraceFlags::SAMPLED,
            false,
            TraceState::default(),
        ))
    }
}

pub(crate) struct RecordingSpan(SpanContext);

impl Span for RecordingSpan {
    fn add_event_with_timestamp<T>(&mut self, _: T, _: SystemTime, _: Vec<KeyValue>)
    where
        T: Into<Cow<'static, str>>,
    {
    }
    fn span_context(&self) -> &SpanContext {
        &self.0
    }
    fn is_recording(&self) -> bool {
        true
    }
    fn set_attribute(&mut self, _: KeyValue) {}
    fn set_status(&mut self, _: Status) {}
    fn update_name<T>(&mut self, _: T)
    where
        T: Into<Cow<'static, str>>,
    {
    }
    fn add_link(&mut self, _: SpanContext, _: Vec<KeyValue>) {}
    fn end_with_timestamp(&mut self, _: SystemTime) {}
}
