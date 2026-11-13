//! Poll-local and destruction-local context activation, without unsafe projection.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::InstrumentationScope;

/// Boxes SDK-owned work, activating context only when a scope is configured.
#[doc(hidden)]
pub fn instrument_future<'a, F>(
    scope: Option<InstrumentationScope>,
    future: F,
) -> crate::executor::BoxFuture<'a, F::Output>
where
    F: Future + Send + 'a,
{
    match scope {
        Some(scope) => Box::pin(scope.instrument(future)),
        None => Box::pin(future),
    }
}

/// Future retaining its execution scope until owned future destruction.
///
/// No thread-local guard is stored in this value. The scope and pinned future
/// may move between threads whenever the inner future is `Send`.
pub struct InstrumentedFuture<F: Future> {
    scope: InstrumentationScope,
    future: Option<Pin<Box<F>>>,
}

impl<F: Future> InstrumentedFuture<F> {
    pub(super) fn new(scope: InstrumentationScope, future: F) -> Self {
        Self {
            scope,
            future: Some(Box::pin(future)),
        }
    }
}

impl<F: Future> Future for InstrumentedFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let scope = this.scope.clone();
        scope.run(|| {
            this.future
                .as_mut()
                .expect("an instrumented future owns its inner future until destruction")
                .as_mut()
                .poll(context)
        })
    }
}

impl<F: Future> Drop for InstrumentedFuture<F> {
    fn drop(&mut self) {
        let future = self.future.take();
        self.scope.run(|| drop(future));
    }
}
