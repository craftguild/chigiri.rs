//! End notification and actual released SDK span destruction are separate probes.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use opentelemetry::KeyValue;
use opentelemetry::trace::{Span, SpanContext, Status};

pub(crate) struct CountedSpan {
    inner: Option<opentelemetry_sdk::trace::Span>,
    dropped: Arc<AtomicUsize>,
}
impl CountedSpan {
    pub(super) fn new(inner: opentelemetry_sdk::trace::Span, dropped: Arc<AtomicUsize>) -> Self {
        Self {
            inner: Some(inner),
            dropped,
        }
    }
}
impl Span for CountedSpan {
    fn add_event_with_timestamp<T: Into<Cow<'static, str>>>(
        &mut self,
        name: T,
        timestamp: SystemTime,
        attributes: Vec<KeyValue>,
    ) {
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
    fn update_name<T: Into<Cow<'static, str>>>(&mut self, name: T) {
        self.inner.as_mut().unwrap().update_name(name);
    }
    fn add_link(&mut self, context: SpanContext, attributes: Vec<KeyValue>) {
        self.inner.as_mut().unwrap().add_link(context, attributes);
    }
    fn end_with_timestamp(&mut self, timestamp: SystemTime) {
        self.inner.as_mut().unwrap().end_with_timestamp(timestamp);
    }
}
impl Drop for CountedSpan {
    fn drop(&mut self) {
        drop(self.inner.take());
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
