//! Binds provider-failure observation to the finalized component graph.

use std::sync::{Arc, Mutex};

use chigiri_runtime::ErrorObserver;

/// Connects the final error observer to Stage A and Stage B wrappers.
pub trait RateLimitObserverBinding: Send + Sync + 'static {
    /// Replaces the observer with the final selection from the component graph.
    fn set_error_observer(&self, observer: Option<Arc<dyn ErrorObserver>>);
}

#[derive(Clone, Default)]
pub(crate) struct ObserverSlot {
    observer: Arc<Mutex<Option<Arc<dyn ErrorObserver>>>>,
}

impl ObserverSlot {
    pub(crate) fn set(&self, observer: Option<Arc<dyn ErrorObserver>>) {
        *self
            .observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = observer;
    }

    pub(crate) fn get(&self) -> Option<Arc<dyn ErrorObserver>> {
        self.observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
