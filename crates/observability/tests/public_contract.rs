use chigiri_observability::{DeterministicErrorIdFactory, MetricLabelKey};
use chigiri_runtime::{ErrorObserver, LifecycleObserver, RequestId, TraceId};

#[test]
fn observer_is_object_safe_and_distinct_from_interceptors() {
    let lifecycle: Option<&dyn LifecycleObserver> = None;
    let errors: Option<&dyn ErrorObserver> = None;
    assert!(lifecycle.is_none() && errors.is_none());
    assert_ne!(
        std::any::TypeId::of::<RequestId>(),
        std::any::TypeId::of::<TraceId>()
    );
    let _ = DeterministicErrorIdFactory::new(1);
    let _ = MetricLabelKey::LifecyclePhase;
}
