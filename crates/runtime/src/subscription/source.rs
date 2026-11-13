use std::pin::Pin;

use futures_core::Stream;

type ErasedSubscriptionStream<Event, Error> =
    Pin<Box<dyn Stream<Item = Result<Event, Error>> + Send + 'static>>;

/// The bounded-queue behavior selected by the server for a subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionOverflow {
    /// Stop polling the source until delivery capacity becomes available.
    Backpressure,
    /// Discard the oldest whole queued event before admitting a new event.
    DropOldest,
}

/// An owned, type-erased source of subscription events.
///
/// The source is pinned when it enters the wrapper and can only be consumed by
/// the runtime. It cannot borrow request setup data, be cloned, or be polled
/// through the public API.
///
/// A subscription event must be both `Send` and `Sync`:
///
/// ```compile_fail
/// use std::cell::Cell;
/// use chigiri_runtime::SubscriptionSource;
///
/// fn requires_sync(_: Option<SubscriptionSource<Cell<u8>, ()>>) {}
/// ```
///
/// A subscription error must be `Send`:
///
/// ```compile_fail
/// use std::rc::Rc;
/// use chigiri_runtime::SubscriptionSource;
///
/// fn requires_send(_: Option<SubscriptionSource<u8, Rc<u8>>>) {}
/// ```
///
/// The stream must own all data that it retains:
///
/// ```compile_fail
/// use std::pin::Pin;
/// use std::task::{Context, Poll};
/// use chigiri_runtime::SubscriptionSource;
/// use futures_core::Stream;
///
/// struct Borrowed<'a>(&'a u8);
///
/// impl Stream for Borrowed<'_> {
///     type Item = Result<u8, ()>;
///
///     fn poll_next(
///         self: Pin<&mut Self>,
///         _context: &mut Context<'_>,
///     ) -> Poll<Option<Self::Item>> {
///         Poll::Ready(Some(Ok(*self.as_ref().get_ref().0)))
///     }
/// }
///
/// let value = 1;
/// let _source = SubscriptionSource::new(Borrowed(&value));
/// ```
///
/// The wrapper deliberately has no `Clone` implementation:
///
/// ```compile_fail
/// use chigiri_runtime::SubscriptionSource;
///
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<SubscriptionSource<u8, ()>>();
/// ```
///
/// The erased stream cannot escape through a public `Stream` implementation:
///
/// ```compile_fail
/// use chigiri_runtime::SubscriptionSource;
/// use futures_core::Stream;
///
/// fn requires_stream<T: Stream>() {}
/// requires_stream::<SubscriptionSource<u8, ()>>();
/// ```
///
/// It also exposes no public polling method:
///
/// ```compile_fail
/// use std::pin::Pin;
/// use std::task::Context;
/// use chigiri_runtime::SubscriptionSource;
/// use futures_core::Stream;
///
/// fn poll_publicly(
///     source: &mut SubscriptionSource<u8, ()>,
///     context: &mut Context<'_>,
/// ) {
///     let _ = Pin::new(source).poll_next(context);
/// }
/// ```
pub struct SubscriptionSource<Event: Send + Sync + 'static, Error: Send + 'static> {
    stream: ErasedSubscriptionStream<Event, Error>,
    overflow: SubscriptionOverflow,
}

impl<Event: Send + Sync + 'static, Error: Send + 'static> SubscriptionSource<Event, Error> {
    /// Owns and pins a subscription stream using backpressure by default.
    pub fn new<Source>(stream: Source) -> Self
    where
        Source: Stream<Item = Result<Event, Error>> + Send + 'static,
    {
        Self {
            stream: Box::pin(stream),
            overflow: SubscriptionOverflow::Backpressure,
        }
    }

    /// Selects the server-side overflow behavior for this subscription.
    pub fn with_overflow(mut self, overflow: SubscriptionOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    pub(crate) fn into_parts(
        self,
    ) -> (ErasedSubscriptionStream<Event, Error>, SubscriptionOverflow) {
        (self.stream, self.overflow)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    use futures_core::Stream;

    use super::{SubscriptionOverflow, SubscriptionSource};

    struct Values(VecDeque<Result<u8, &'static str>>);

    impl Stream for Values {
        type Item = Result<u8, &'static str>;

        fn poll_next(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
        ) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.pop_front())
        }
    }

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    #[test]
    fn runtime_consumes_the_erased_pinned_stream_and_overflow_policy() {
        let source = SubscriptionSource::new(Values(VecDeque::from([Ok(7), Err("source failed")])))
            .with_overflow(SubscriptionOverflow::DropOldest);

        let (mut stream, overflow) = source.into_parts();
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);

        assert_eq!(overflow, SubscriptionOverflow::DropOldest);
        assert_eq!(
            stream.as_mut().poll_next(&mut context),
            Poll::Ready(Some(Ok(7)))
        );
        assert_eq!(
            stream.as_mut().poll_next(&mut context),
            Poll::Ready(Some(Err("source failed")))
        );
        assert_eq!(stream.as_mut().poll_next(&mut context), Poll::Ready(None));
    }

    #[test]
    fn backpressure_is_the_default() {
        let source = SubscriptionSource::new(Values(VecDeque::new()));
        let (_, overflow) = source.into_parts();

        assert_eq!(overflow, SubscriptionOverflow::Backpressure);
    }
}
