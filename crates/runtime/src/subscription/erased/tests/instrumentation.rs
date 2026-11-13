//! Typed argument/event destruction binds selection without retaining TLS guards.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_core::instrumentation::{
    ExecutionScope, InstrumentationGuard, InstrumentationKind, InstrumentationScope,
    PropagationContext,
};
use chigiri_core::{ErrorEvent, LifecycleResult, SamplingDecision};

use super::{ErasedSubscriptionArguments, ErasedSubscriptionEvent, ScopedValue};

thread_local! { static ACTIVE: Cell<u8> = const { Cell::new(0) }; }

struct Scope(u8);
struct Guard(u8);
impl InstrumentationGuard for Guard {}
impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.set(self.0);
    }
}
impl ExecutionScope for Scope {
    fn enter(&self) -> Box<dyn InstrumentationGuard + '_> {
        Box::new(Guard(ACTIVE.replace(self.0)))
    }
    fn context(&self) -> PropagationContext {
        PropagationContext::default()
    }
    fn child(&self, _: InstrumentationKind, _: SamplingDecision) -> InstrumentationScope {
        InstrumentationScope::new(Scope(self.0))
    }
    fn observe_error(&self, _: &ErrorEvent<'_>) {}
    fn finish(&self, _: LifecycleResult) {}
}

struct Marker {
    expected: u8,
    drops: Arc<AtomicUsize>,
}
impl Drop for Marker {
    fn drop(&mut self) {
        assert_eq!(ACTIVE.get(), self.expected);
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn retained_arguments_drop_under_selected_owner_and_restore_the_worker() {
    let drops = Arc::new(AtomicUsize::new(0));
    let scope = InstrumentationScope::new(Scope(42));
    let arguments = ErasedSubscriptionArguments(ScopedValue::new(Marker {
        expected: 42,
        drops: Arc::clone(&drops),
    }))
    .with_scope(Some(scope));
    let worker = Scope(99);
    let _guard = worker.enter();
    assert_eq!(arguments.downcast_ref::<Marker>().unwrap().expected, 42);
    drop(arguments);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(ACTIVE.get(), 99);
}

#[test]
fn admitted_event_replaces_source_destruction_scope_and_drops_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let source = InstrumentationScope::new(Scope(42));
    let event = InstrumentationScope::new(Scope(84));
    let value = ErasedSubscriptionEvent(ScopedValue::new(Marker {
        expected: 84,
        drops: Arc::clone(&drops),
    }))
    .with_scope(Some(source))
    .with_scope(Some(event));
    let worker = Scope(99);
    let _guard = worker.enter();
    drop(value);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(ACTIVE.get(), 99);
}

#[test]
fn value_drop_unwind_restores_context_without_retrying_the_payload() {
    struct FailingDrop(Arc<AtomicUsize>);
    impl Drop for FailingDrop {
        fn drop(&mut self) {
            assert_eq!(ACTIVE.get(), 42);
            assert_eq!(self.0.fetch_add(1, Ordering::SeqCst), 0);
            panic!("owned value disposal failure fixture");
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let arguments = ErasedSubscriptionArguments(ScopedValue::new(FailingDrop(Arc::clone(&drops))))
        .with_scope(Some(InstrumentationScope::new(Scope(42))));
    let worker = Scope(99);
    let _guard = worker.enter();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(arguments)));
    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(ACTIVE.get(), 99);
}
