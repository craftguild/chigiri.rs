//! Raw context futures retain successful values across their own failed Drop.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::{BoxFuture, PrepareError, PreparedContext};

use super::ErasedPreparedContext;
use crate::managed::EventActivity;
use crate::subscription::worker::dispose_value;
use crate::transport::deadline::HttpDeadlineMonitor;

/// Only the two existing SDK context-task roles may report disposal integrity.
#[derive(Clone)]
pub(crate) enum ContextTaskScope {
    Subscription(EventActivity),
    Http(HttpDeadlineMonitor),
}

impl ContextTaskScope {
    pub(crate) fn subscription(activity: Option<EventActivity>) -> Option<Self> {
        activity
            .filter(EventActivity::is_managed)
            .map(Self::Subscription)
    }

    pub(crate) fn http(monitor: &HttpDeadlineMonitor) -> Option<Self> {
        monitor.is_managed().then(|| Self::Http(monitor.clone()))
    }

    pub(crate) fn failed(&self) -> bool {
        match self {
            Self::Subscription(activity) => activity.task_failed(),
            Self::Http(monitor) => monitor.faulted(),
        }
    }

    fn poison(&self) {
        match self {
            Self::Subscription(activity) => activity.poison_task(),
            Self::Http(monitor) => monitor.poison(),
        }
    }

    fn dispose<T>(&self, value: T) {
        if !dispose_value(value) {
            self.poison();
        }
    }

    pub(crate) fn dispose_preparation<C>(
        &self,
        prepared: Result<ErasedPreparedContext<C>, PrepareError>,
    ) {
        match prepared {
            Ok(prepared) => {
                let (context, state, completion) = prepared.into_parts();
                // Independent fields must not unwind through one another.
                self.dispose(context);
                self.dispose(state);
                self.dispose(completion);
            }
            Err(error) => self.dispose(error),
        }
    }
}

pub(super) struct ScopedContextFuture<'a, C, S> {
    future: Option<BoxFuture<'a, Result<PreparedContext<C, S>, PrepareError>>>,
    scope: ContextTaskScope,
}

impl<'a, C, S> ScopedContextFuture<'a, C, S> {
    pub(super) fn new(
        future: BoxFuture<'a, Result<PreparedContext<C, S>, PrepareError>>,
        scope: ContextTaskScope,
    ) -> Self {
        Self {
            future: Some(future),
            scope,
        }
    }
}

impl<C, S> Future for ScopedContextFuture<'_, C, S> {
    type Output = Result<PreparedContext<C, S>, PrepareError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.future
            .as_mut()
            .expect("live raw context future")
            .as_mut()
            .poll(cx)
    }
}

impl<C, S> Drop for ScopedContextFuture<'_, C, S> {
    fn drop(&mut self) {
        if let Some(future) = self.future.take() {
            self.scope.dispose(future);
        }
    }
}
