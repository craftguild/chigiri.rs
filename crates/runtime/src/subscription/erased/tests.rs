use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use chigiri_core::FieldError;
use futures_core::Stream;

use super::super::{SubscriptionOverflow, SubscriptionSource};
use super::ScopedValue;
use super::{ErasedSubscriptionArguments, ErasedSubscriptionEvent, erase_subscription_source};

#[path = "tests/instrumentation.rs"]
mod instrumentation;

type SourceItem = Result<u8, Cell<u8>>;

struct Source {
    values: RefCell<VecDeque<Option<SourceItem>>>,
    polls: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
    _pinned: PhantomPinned,
}

impl Stream for Source {
    type Item = Result<u8, Cell<u8>>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_ref().get_ref();
        this.polls.fetch_add(1, Ordering::SeqCst);
        match this.values.borrow_mut().pop_front() {
            Some(Some(value)) => Poll::Ready(Some(value)),
            Some(None) => {
                context.waker().wake_by_ref();
                Poll::Pending
            }
            None => Poll::Ready(None),
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

struct Signal;

impl Wake for Signal {
    fn wake(self: Arc<Self>) {}
}

fn source(
    values: impl IntoIterator<Item = Option<Result<u8, Cell<u8>>>>,
) -> (
    SubscriptionSource<u8, Cell<u8>>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
) {
    let polls = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let source = SubscriptionSource::new(Source {
        values: RefCell::new(values.into_iter().collect()),
        polls: Arc::clone(&polls),
        dropped: Arc::clone(&dropped),
        _pinned: PhantomPinned,
    });
    (source, polls, dropped)
}

fn mapper(calls: &Arc<AtomicUsize>) -> Arc<dyn Fn(Cell<u8>) -> FieldError + Send + Sync> {
    let calls = Arc::clone(calls);
    Arc::new(move |error| {
        calls.fetch_add(1, Ordering::SeqCst);
        FieldError::public(format!("source error {}", error.get())).code("SOURCE_FAILED")
    })
}

#[test]
fn source_is_demand_driven_and_maps_a_terminal_error_once() {
    let (source, polls, dropped) = source([
        None,
        Some(Ok(7)),
        Some(Err(Cell::new(8))),
        Some(Err(Cell::new(9))),
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let erased = erase_subscription_source(
        source.with_overflow(SubscriptionOverflow::DropOldest),
        "retained arguments".to_owned(),
        mapper(&calls),
    );
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (mut stream, arguments, overflow) = erased.into_parts();
    assert_eq!(overflow, SubscriptionOverflow::DropOldest);
    assert_eq!(
        arguments.downcast_ref::<String>().unwrap(),
        "retained arguments"
    );
    let waker = Waker::from(Arc::new(Signal));
    let mut context = Context::from_waker(&waker);
    assert!(stream.as_mut().poll_next(&mut context).is_pending());
    let Poll::Ready(Some(Ok(event))) = stream.as_mut().poll_next(&mut context) else {
        panic!("expected a source event")
    };
    assert_eq!(*event.downcast_ref::<u8>().unwrap(), 7);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let Poll::Ready(Some(Err(error))) = stream.as_mut().poll_next(&mut context) else {
        panic!("expected a terminal source error")
    };
    assert_eq!(
        error.safe_summary(),
        ("source error 8", Some("SOURCE_FAILED"))
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for _ in 0..2 {
        assert!(matches!(
            stream.as_mut().poll_next(&mut context),
            Poll::Ready(None)
        ));
    }
    assert_eq!(polls.load(Ordering::SeqCst), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn successful_completion_releases_the_source_without_mapping() {
    let (source, polls, dropped) = source([]);
    let calls = Arc::new(AtomicUsize::new(0));
    let (mut stream, _, overflow) =
        erase_subscription_source(source, (), mapper(&calls)).into_parts();
    let waker = Waker::from(Arc::new(Signal));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(
        stream.as_mut().poll_next(&mut context),
        Poll::Ready(None)
    ));
    assert!(matches!(
        stream.as_mut().poll_next(&mut context),
        Poll::Ready(None)
    ));
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(overflow, SubscriptionOverflow::Backpressure);
}

#[test]
fn drop_before_demand_releases_source_and_arguments_without_mapping() {
    struct Arguments(Arc<AtomicBool>);
    impl Drop for Arguments {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let (source, polls, source_dropped) = source([Some(Ok(1))]);
    let arguments_dropped = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let erased = erase_subscription_source(
        source,
        Arguments(Arc::clone(&arguments_dropped)),
        mapper(&calls),
    );
    fn require_send<T: Send>(_: &T) {}
    require_send(&erased);
    drop(erased);
    assert!(source_dropped.load(Ordering::SeqCst));
    assert!(arguments_dropped.load(Ordering::SeqCst));
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn type_mismatches_are_masked_framework_errors() {
    let arguments = ErasedSubscriptionArguments(ScopedValue::new(7_u8));
    let event = ErasedSubscriptionEvent(ScopedValue::new("event".to_owned()));
    for error in [
        arguments.downcast_ref::<u64>().unwrap_err(),
        event.downcast_ref::<u64>().unwrap_err(),
    ] {
        assert!(error.is_internal());
        assert_eq!(
            error.safe_summary(),
            ("Internal server error", Some("INTERNAL_SERVER_ERROR"))
        );
    }
    assert_eq!(*arguments.downcast_ref::<u8>().unwrap(), 7);
    assert_eq!(event.downcast_ref::<String>().unwrap(), "event");
    fn require_sync<T: Send + Sync>(_: &T) {}
    require_sync(&arguments);
    require_sync(&event);
}
