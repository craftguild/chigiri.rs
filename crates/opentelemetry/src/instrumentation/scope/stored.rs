//! Cache ownership is separate from a transient synchronous activation.

use opentelemetry::Context;
use std::ops::Deref;

pub(super) struct StoredContext {
    inner: Context,
    #[cfg(test)]
    retained: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl StoredContext {
    pub(super) fn new<T>(inner: Context, _shared: &super::Shared<T>) -> Self {
        #[cfg(test)]
        _shared
            .retained_contexts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self {
            inner,
            #[cfg(test)]
            retained: std::sync::Arc::clone(&_shared.retained_contexts),
        }
    }
}

impl Deref for StoredContext {
    type Target = Context;
    fn deref(&self) -> &Context {
        &self.inner
    }
}

#[cfg(test)]
impl Drop for StoredContext {
    fn drop(&mut self) {
        self.retained
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}
