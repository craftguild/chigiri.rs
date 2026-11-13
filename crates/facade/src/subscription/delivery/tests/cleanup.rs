use std::future::{Future, pending};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use super::settings;
use crate::subscription::delivery::{
    CancellationFlag, DeliveryResources, EmissionAuthority, QuarantineError, QuarantineKey,
    QuarantineRegistry,
};

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn context() -> Context<'static> {
    let waker = Waker::from(Arc::new(NoopWake));
    Context::from_waker(Box::leak(Box::new(waker)))
}

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

struct PendingOnce {
    polled: bool,
}

struct PanicOnPoll(Arc<AtomicUsize>);

impl Future for PanicOnPoll {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        panic!("cleanup poll panic")
    }
}

impl Drop for PanicOnPoll {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

struct ReadyDropPanic(Arc<AtomicUsize>);

impl Future for ReadyDropPanic {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(())
    }
}

impl Drop for ReadyDropPanic {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        panic!("cleanup destructor panic")
    }
}

impl Future for PendingOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        if self.polled {
            Poll::Ready(())
        } else {
            self.polled = true;
            Poll::Pending
        }
    }
}

#[test]
fn logical_revocation_is_immediate_and_generation_specific() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let old = resources.try_pending_operation().unwrap();
    let old_generation = old.generation();
    drop(old);
    let new = resources.try_pending_operation().unwrap();
    let new_generation = new.generation();
    drop(new);

    let authority = EmissionAuthority::new(old_generation);
    assert!(authority.can_emit(old_generation));
    assert!(!authority.can_emit(new_generation));
    authority.revoke();
    assert!(!authority.can_emit(old_generation));

    let cancellation = CancellationFlag::default();
    assert!(!cancellation.is_cancelled());
    cancellation.cancel();
    assert!(cancellation.is_cancelled());
}

#[test]
fn quarantine_retains_charge_until_physical_completion() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let generation = resources.try_pending_operation().unwrap().generation();
    let registry = QuarantineRegistry::new(1);
    let drops = Arc::new(AtomicUsize::new(0));
    let key = QuarantineKey::new(generation);
    assert!(
        registry
            .insert(
                key.clone(),
                PendingOnce { polled: false },
                DropProbe(Arc::clone(&drops)),
            )
            .is_ok()
    );
    assert_eq!(registry.len(), Ok(1));

    let mut context = context();
    assert!(registry.poll_one(&mut context).is_pending());
    assert_eq!(registry.len(), Ok(1));
    assert_eq!(drops.load(Ordering::Acquire), 0);
    assert_eq!(registry.poll_one(&mut context), Poll::Ready(Ok(Some(key))));
    assert_eq!(registry.len(), Ok(0));
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(registry.poll_one(&mut context), Poll::Ready(Ok(None)));
    assert_eq!(drops.load(Ordering::Acquire), 1);
}

#[test]
fn failed_quarantine_insert_returns_the_unreleased_charge() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let generation = resources.try_pending_operation().unwrap().generation();
    let registry = QuarantineRegistry::new(1);
    let key = QuarantineKey::new(generation);
    assert!(registry.insert(key.clone(), pending(), ()).is_ok());

    let drops = Arc::new(AtomicUsize::new(0));
    let failure = registry
        .insert(key, pending(), DropProbe(Arc::clone(&drops)))
        .unwrap_err();
    assert_eq!(failure.error(), QuarantineError::Capacity);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    let (_, future, charge) = failure.into_parts();
    drop(future);
    drop(charge);
    assert_eq!(drops.load(Ordering::Acquire), 1);
}

struct ReentrantPoll {
    registry: QuarantineRegistry,
}

impl Future for ReentrantPoll {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
        assert_eq!(self.registry.len(), Ok(1));
        Poll::Ready(())
    }
}

#[test]
fn quarantine_never_holds_its_lock_while_polling_user_work() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let generation = resources.try_pending_operation().unwrap().generation();
    let registry = QuarantineRegistry::new(1);
    let key = QuarantineKey::new(generation);
    assert!(
        registry
            .insert(
                key.clone(),
                ReentrantPoll {
                    registry: registry.clone(),
                },
                (),
            )
            .is_ok()
    );
    let mut context = context();
    assert_eq!(registry.poll_one(&mut context), Poll::Ready(Ok(Some(key))));
}

#[test]
fn pending_cleanup_does_not_starve_another_generation() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let first = resources.try_pending_operation().unwrap().generation();
    let second = resources.try_pending_operation().unwrap().generation();
    let third = resources.try_pending_operation().unwrap().generation();
    let registry = QuarantineRegistry::new(3);
    let first_key = QuarantineKey::new(first);
    let second_key = QuarantineKey::new(second);
    let third_key = QuarantineKey::new(third);
    let first_drops = Arc::new(AtomicUsize::new(0));
    let second_drops = Arc::new(AtomicUsize::new(0));
    let third_drops = Arc::new(AtomicUsize::new(0));
    assert!(
        registry
            .insert(first_key, pending(), DropProbe(Arc::clone(&first_drops)),)
            .is_ok()
    );
    assert!(
        registry
            .insert(
                second_key.clone(),
                async {},
                DropProbe(Arc::clone(&second_drops)),
            )
            .is_ok()
    );
    assert!(
        registry
            .insert(
                third_key.clone(),
                async {},
                DropProbe(Arc::clone(&third_drops)),
            )
            .is_ok()
    );

    let mut context = context();
    assert!(registry.poll_one(&mut context).is_pending());
    assert_eq!(
        registry.poll_one(&mut context),
        Poll::Ready(Ok(Some(second_key)))
    );
    assert_eq!(second_drops.load(Ordering::Acquire), 1);
    assert_eq!(first_drops.load(Ordering::Acquire), 0);
    assert_eq!(registry.len(), Ok(2));
    assert_eq!(
        registry.poll_one(&mut context),
        Poll::Ready(Ok(Some(third_key)))
    );
    assert_eq!(third_drops.load(Ordering::Acquire), 1);
    assert_eq!(registry.len(), Ok(1));
    assert!(registry.poll_one(&mut context).is_pending());
    assert_eq!(first_drops.load(Ordering::Acquire), 0);
}

#[test]
fn panicking_poll_retains_work_and_charge_in_a_poisoned_slot() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let generation = resources.try_pending_operation().unwrap().generation();
    let registry = QuarantineRegistry::new(1);
    let future_drops = Arc::new(AtomicUsize::new(0));
    let charge_drops = Arc::new(AtomicUsize::new(0));
    assert!(
        registry
            .insert(
                QuarantineKey::new(generation),
                PanicOnPoll(Arc::clone(&future_drops)),
                DropProbe(Arc::clone(&charge_drops)),
            )
            .is_ok()
    );

    let mut context = context();
    let panic = catch_unwind(AssertUnwindSafe(|| registry.poll_one(&mut context)));
    assert!(panic.is_err());
    assert_eq!(future_drops.load(Ordering::Acquire), 0);
    assert_eq!(charge_drops.load(Ordering::Acquire), 0);
    assert_eq!(registry.retained_slots(), 1);
    assert_eq!(registry.len(), Err(QuarantineError::Invariant));
    assert_eq!(
        registry.poll_one(&mut context),
        Poll::Ready(Err(QuarantineError::Invariant))
    );

    drop(registry);
    assert_eq!(future_drops.load(Ordering::Acquire), 1);
    assert_eq!(charge_drops.load(Ordering::Acquire), 1);
}

#[test]
fn ready_future_destructor_panic_retains_charge_until_next_driver_poll() {
    let resources = DeliveryResources::new(&settings()).unwrap();
    let generation = resources.try_pending_operation().unwrap().generation();
    let registry = QuarantineRegistry::new(1);
    let key = QuarantineKey::new(generation);
    let future_drops = Arc::new(AtomicUsize::new(0));
    let charge_drops = Arc::new(AtomicUsize::new(0));
    assert!(
        registry
            .insert(
                key.clone(),
                ReadyDropPanic(Arc::clone(&future_drops)),
                DropProbe(Arc::clone(&charge_drops)),
            )
            .is_ok()
    );

    let mut context = context();
    let panic = catch_unwind(AssertUnwindSafe(|| registry.poll_one(&mut context)));
    assert!(panic.is_err());
    assert_eq!(future_drops.load(Ordering::Acquire), 1);
    assert_eq!(charge_drops.load(Ordering::Acquire), 0);
    assert_eq!(registry.retained_slots(), 1);
    assert_eq!(registry.len(), Ok(1));

    assert_eq!(registry.poll_one(&mut context), Poll::Ready(Ok(Some(key))));
    assert_eq!(future_drops.load(Ordering::Acquire), 1);
    assert_eq!(charge_drops.load(Ordering::Acquire), 1);
    assert_eq!(registry.retained_slots(), 0);
    assert_eq!(registry.poll_one(&mut context), Poll::Ready(Ok(None)));
}
