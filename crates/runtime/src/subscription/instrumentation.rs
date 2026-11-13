//! Source waiting owns only detached subscriber context, never a live setup span.

use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_core::FieldResult;
use chigiri_core::instrumentation::InstrumentationScope;
use futures_core::Stream;

use super::erased::{ErasedEventStream, ErasedSubscriptionEvent, EventStream};

pub(super) fn instrument_source(
    source: EventStream,
    scope: Option<InstrumentationScope>,
) -> EventStream {
    match scope {
        Some(scope) => Box::pin(SubscriberSource {
            source: Some(source),
            scope,
        }),
        None => source,
    }
}

struct SubscriberSource {
    source: Option<EventStream>,
    scope: InstrumentationScope,
}

impl Stream for SubscriberSource {
    type Item = FieldResult<ErasedSubscriptionEvent>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.scope.run(|| {
            this.source
                .as_mut()
                .expect("subscriber source owns its stream until destruction")
                .as_mut()
                .poll_next(context)
                .map(|item| {
                    item.map(|event| event.map(|event| event.with_scope(Some(this.scope.clone()))))
                })
        })
    }
}

impl ErasedEventStream for SubscriberSource {
    fn defer_destruction(self: Pin<&mut Self>) {
        let this = self.get_mut();
        this.scope.run(|| {
            if let Some(source) = this.source.as_mut() {
                source.as_mut().defer_destruction();
            }
        });
    }
}

impl Drop for SubscriberSource {
    fn drop(&mut self) {
        let source = self.source.take();
        self.scope.run(|| drop(source));
    }
}
