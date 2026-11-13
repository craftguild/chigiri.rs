//! Runtime-owned stream erasure used by generated subscription adapters.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use chigiri_core::instrumentation::InstrumentationScope;
use chigiri_core::{FieldError, FieldResult};
use futures_core::Stream;

use super::{SubscriptionOverflow, SubscriptionSource};

#[path = "erased/value.rs"]
mod value;
use value::ScopedValue;

pub(super) type EventStream = Pin<Box<dyn ErasedEventStream>>;

pub(crate) trait ErasedEventStream:
    Stream<Item = FieldResult<ErasedSubscriptionEvent>> + Send + 'static
{
    fn defer_destruction(self: Pin<&mut Self>);
}

/// Owned typed arguments retained after source initialization.
#[doc(hidden)]
pub struct ErasedSubscriptionArguments(ScopedValue);

impl ErasedSubscriptionArguments {
    pub(super) fn with_scope(mut self, scope: Option<InstrumentationScope>) -> Self {
        self.0.bind(scope);
        self
    }
    /// Borrows the generated argument type without repeating input coercion.
    pub fn downcast_ref<T: 'static>(&self) -> FieldResult<&T> {
        self.0.downcast_ref().ok_or_else(|| {
            crate::private::invariant_error("subscription argument application type does not match")
        })
    }
}

/// One owned source event passed to its generated event adapter.
#[doc(hidden)]
pub struct ErasedSubscriptionEvent(ScopedValue);

impl ErasedSubscriptionEvent {
    pub(super) fn with_scope(mut self, scope: Option<InstrumentationScope>) -> Self {
        self.0.bind(scope);
        self
    }
    /// Borrows the application event using the generated coordinate's type.
    pub fn downcast_ref<T: 'static>(&self) -> FieldResult<&T> {
        self.0.downcast_ref().ok_or_else(|| {
            crate::private::invariant_error("subscription event application type does not match")
        })
    }
}

/// Source ownership returned by a generated setup adapter.
#[doc(hidden)]
pub struct ErasedSubscription {
    stream: EventStream,
    arguments: ErasedSubscriptionArguments,
    overflow: SubscriptionOverflow,
    observation: Option<super::SubscriptionObservationReceipt>,
}

impl ErasedSubscription {
    /// Attaches only the opaque receipt issued to the same host's preparation.
    /// It carries neither a sink nor a caller-supplied Mock generation.
    #[doc(hidden)]
    pub fn with_host_observation(
        mut self,
        receipt: super::SubscriptionObservationReceipt,
    ) -> FieldResult<Self> {
        if self.observation.is_some() || receipt.is_closed() {
            return Err(crate::private::invariant_error(
                "subscription observation receipt is closed or already attached",
            ));
        }
        self.observation = Some(receipt);
        self.stream.as_mut().defer_destruction();
        Ok(self)
    }

    pub(crate) fn host_observation_receipt(
        &self,
    ) -> Option<&super::SubscriptionObservationReceipt> {
        self.observation.as_ref()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        EventStream,
        ErasedSubscriptionArguments,
        SubscriptionOverflow,
    ) {
        (self.stream, self.arguments, self.overflow)
    }
}

/// Retains typed arguments and lazily maps only source-item application errors.
#[doc(hidden)]
pub fn erase_subscription_source<Event, Error, Arguments>(
    source: SubscriptionSource<Event, Error>,
    arguments: Arguments,
    mapper: Arc<dyn Fn(Error) -> FieldError + Send + Sync>,
) -> ErasedSubscription
where
    Event: Send + Sync + 'static,
    Error: Send + 'static,
    Arguments: Send + Sync + 'static,
{
    let (stream, overflow) = source.into_parts();
    ErasedSubscription {
        stream: Box::pin(MappedStream {
            stream: Some(stream),
            mapper,
            ended: false,
            defer_destruction: false,
        }),
        arguments: ErasedSubscriptionArguments(ScopedValue::new(arguments)),
        overflow,
        observation: None,
    }
}

type SourceStream<Event, Error> =
    Pin<Box<dyn Stream<Item = Result<Event, Error>> + Send + 'static>>;

struct MappedStream<Event, Error> {
    stream: Option<SourceStream<Event, Error>>,
    mapper: Arc<dyn Fn(Error) -> FieldError + Send + Sync>,
    ended: bool,
    defer_destruction: bool,
}

impl<Event, Error> Stream for MappedStream<Event, Error>
where
    Event: Send + Sync + 'static,
    Error: Send + 'static,
{
    type Item = FieldResult<ErasedSubscriptionEvent>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.ended {
            return Poll::Ready(None);
        }
        let Some(stream) = this.stream.as_mut() else {
            return Poll::Ready(None);
        };
        match stream.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(event))) => {
                Poll::Ready(Some(Ok(ErasedSubscriptionEvent(ScopedValue::new(event)))))
            }
            Poll::Ready(Some(Err(error))) => {
                this.ended = true;
                if !this.defer_destruction {
                    this.stream = None;
                }
                Poll::Ready(Some(Err((this.mapper)(error))))
            }
            Poll::Ready(None) => {
                this.ended = true;
                if !this.defer_destruction {
                    this.stream = None;
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<Event, Error> ErasedEventStream for MappedStream<Event, Error>
where
    Event: Send + Sync + 'static,
    Error: Send + 'static,
{
    fn defer_destruction(self: Pin<&mut Self>) {
        self.get_mut().defer_destruction = true;
    }
}

#[cfg(test)]
mod tests;
