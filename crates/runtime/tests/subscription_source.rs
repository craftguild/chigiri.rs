use std::cell::{Cell, RefCell};
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_runtime::{SubscriptionOverflow, SubscriptionSource};
use futures_core::Stream;

struct PinnedOneEvent {
    item: RefCell<Option<Result<Event, SendOnlyError>>>,
    _pinned: PhantomPinned,
}

#[derive(Debug, Eq, PartialEq)]
struct Event(u8);

type SendOnlyError = Cell<u8>;

impl Stream for PinnedOneEvent {
    type Item = Result<Event, SendOnlyError>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.as_ref().get_ref().item.borrow_mut().take())
    }
}

fn consume_owned_source(source: SubscriptionSource<Event, SendOnlyError>) {
    require_send(source);
}

fn require_send<T: Send>(_: T) {}

fn require_overflow_traits<T: Clone + Copy + std::fmt::Debug + Eq + PartialEq>() {}

#[test]
fn public_api_constructs_and_transfers_an_owned_source() {
    let source = SubscriptionSource::new(PinnedOneEvent {
        item: RefCell::new(Some(Ok(Event(1)))),
        _pinned: PhantomPinned,
    })
    .with_overflow(SubscriptionOverflow::DropOldest);

    consume_owned_source(source);
}

#[test]
fn overflow_values_have_the_frozen_value_traits() {
    require_overflow_traits::<SubscriptionOverflow>();
    let value = SubscriptionOverflow::Backpressure;

    assert_eq!(value, SubscriptionOverflow::Backpressure);
    assert_eq!(format!("{value:?}"), "Backpressure");
}
