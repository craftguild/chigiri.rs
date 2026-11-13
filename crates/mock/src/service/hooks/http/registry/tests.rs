//! Scope correlation cannot retain a graph or redirect an expired identity.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

struct ApplicationCapture(Arc<AtomicUsize>);
impl Drop for ApplicationCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn original_admission_follows_the_scope_and_weak_reuse_cannot_alias_old_identity() {
    let registry = HttpScopes::new().unwrap();
    let admission = Arc::new(tokio::sync::Semaphore::new(1));
    let drops = Arc::new(AtomicUsize::new(0));
    let application = ApplicationCapture(drops.clone());
    let first = HttpScope::new("127.0.0.1:49123".parse().unwrap()).with_events(move |_| {
        assert_eq!(application.0.load(Ordering::SeqCst), 0);
    });
    first
        .hold_admission(admission.clone().try_acquire_owned().unwrap())
        .unwrap();
    let identity = registry.bind(&first).unwrap();
    let retained = registry.scope(Some(&identity)).unwrap();
    assert_eq!(retained.request_id(), first.request_id());
    drop(first);
    assert!(admission.clone().try_acquire_owned().is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(retained);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let next = HttpScope::new("127.0.0.1:49124".parse().unwrap());
    assert!(registry.bind(&next).is_err());
    next.hold_admission(admission.try_acquire_owned().unwrap())
        .unwrap();
    let next_identity = registry.bind(&next).unwrap();
    assert_ne!(identity, next_identity);
    assert!(registry.scope(Some(&identity)).is_none());
    assert_eq!(
        registry.scope(Some(&next_identity)).unwrap().request_id(),
        next.request_id()
    );
    assert!(registry.scope(None).is_none());
}
