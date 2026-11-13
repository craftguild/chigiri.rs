//! Exercise bounded transfer, closed causes, and once-only outside consumption.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Flush,
    Drop(u64),
    Finish(SubscriptionTermination, u64),
}

struct Host {
    events: Arc<Mutex<Vec<Event>>>,
    panic_finish: bool,
}

impl HostSubscriptionObservation for Host {
    fn retained_metadata_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    fn flush(&mut self) {
        self.events.lock().unwrap().push(Event::Flush);
    }
    fn registered(&self, overflow: SubscriptionOverflow) -> bool {
        overflow == SubscriptionOverflow::DropOldest
    }
    fn drop_oldest(&mut self, total: u64) {
        self.events.lock().unwrap().push(Event::Drop(total));
    }
    fn finish(&mut self, decision: SubscriptionTermination, total: u64) {
        self.events
            .lock()
            .unwrap()
            .push(Event::Finish(decision, total));
        assert!(!self.panic_finish, "host observation test panic");
    }
}

#[derive(Default)]
struct Count(std::sync::atomic::AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn owner(panic_finish: bool) -> (SubscriptionObservation, Arc<Mutex<Vec<Event>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    (
        SubscriptionObservation {
            host: Some(Box::new(Host {
                events: events.clone(),
                panic_finish,
            })),
            receipt: SubscriptionObservationReceipt::new(),
            retained_bytes: 200,
            dropped_total: 0,
        },
        events,
    )
}

#[test]
fn initializer_waits_for_exact_outside_acceptance_and_close_wins_late_acceptance() {
    let (mut owner, _) = owner(false);
    let receipt = owner.receipt.clone();
    let wake = Waker::from(Arc::new(Count::default()));
    let mut context = Context::from_waker(&wake);
    let mut waiting = Box::pin(receipt.wait_for_acceptance());
    assert!(waiting.as_mut().poll(&mut context).is_pending());
    assert!(owner.accept());
    assert_eq!(waiting.as_mut().poll(&mut context), Poll::Ready(true));
    owner.finish(SubscriptionTermination::ClientCancelled, 0);
    assert!(receipt.is_closed());
    assert!(!owner.accept());
    let mut late = Box::pin(receipt.wait_for_acceptance());
    assert_eq!(late.as_mut().poll(&mut context), Poll::Ready(false));
}

#[test]
fn identity_receipts_cannot_substitute_foreign_owners_or_policies() {
    let (owner, _) = owner(false);
    let foreign = SubscriptionObservationReceipt::new();
    let matching = Some(&owner.receipt);
    assert!(owner.registered(matching, SubscriptionOverflow::DropOldest));
    assert!(!owner.registered(matching, SubscriptionOverflow::Backpressure));
    assert!(!owner.registered(Some(&foreign), SubscriptionOverflow::DropOldest));
    assert!(!owner.registered(None, SubscriptionOverflow::DropOldest));
    assert_eq!(owner.retained_metadata_bytes(), 200);
}

#[test]
fn notification_is_coalesced_without_a_task_or_event_queue() {
    let (mut owner, events) = owner(false);
    let count = Arc::new(Count::default());
    let wake = Waker::from(count.clone());
    let mut context = Context::from_waker(&wake);
    owner.poll(&mut context);
    for _ in 0..100 {
        owner.receipt.notify();
    }
    assert_eq!(count.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    owner.poll(&mut context);
    owner.receipt.notify();
    assert_eq!(count.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(*events.lock().unwrap(), vec![Event::Flush, Event::Flush]);
}

#[test]
fn logical_finish_consumes_the_owner_before_cleanup_and_cannot_be_replaced() {
    for selected in [
        SubscriptionTermination::SourceComplete,
        SubscriptionTermination::ExecutionFailed,
        SubscriptionTermination::DeliveryFailed,
        SubscriptionTermination::Internal,
        SubscriptionTermination::ClientCancelled,
        SubscriptionTermination::TransportClosed,
        SubscriptionTermination::ServerShutdown,
    ] {
        let (mut owner, events) = owner(false);
        owner.drop_oldest(1);
        owner.drop_oldest(2);
        owner.finish(selected, 2);
        owner.finish(SubscriptionTermination::ServerShutdown, 9);
        owner.drop_oldest(3);
        drop(owner);
        assert_eq!(
            *events.lock().unwrap(),
            vec![Event::Drop(1), Event::Drop(2), Event::Finish(selected, 2),]
        );
    }
}

#[test]
fn dropped_or_panicking_owner_never_retries_a_terminal() {
    let (mut abandoned, events) = owner(false);
    abandoned.drop_oldest(3);
    drop(abandoned);
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            Event::Drop(3),
            Event::Finish(SubscriptionTermination::Internal, 3),
        ]
    );
    let (mut panicking, events) = owner(true);
    panicking.finish(SubscriptionTermination::ExecutionFailed, 0);
    drop(panicking);
    assert_eq!(
        *events.lock().unwrap(),
        vec![Event::Finish(SubscriptionTermination::ExecutionFailed, 0),]
    );
}

#[test]
fn terminal_mapping_is_closed_and_protocol_independent() {
    for (decision, pair, cancelled) in [
        (
            SubscriptionTermination::SourceComplete,
            ("completed", "source_complete"),
            false,
        ),
        (
            SubscriptionTermination::ExecutionFailed,
            ("failed", "execution_failed"),
            false,
        ),
        (
            SubscriptionTermination::DeliveryFailed,
            ("failed", "delivery_failed"),
            false,
        ),
        (
            SubscriptionTermination::Internal,
            ("failed", "internal"),
            false,
        ),
        (
            SubscriptionTermination::ClientCancelled,
            ("cancelled", "client_cancelled"),
            true,
        ),
        (
            SubscriptionTermination::TransportClosed,
            ("cancelled", "transport_closed"),
            true,
        ),
        (
            SubscriptionTermination::ServerShutdown,
            ("cancelled", "server_shutdown"),
            true,
        ),
    ] {
        assert_eq!(decision.log_pair(), pair);
        assert_eq!(decision.is_cancellation(), cancelled);
    }
}
