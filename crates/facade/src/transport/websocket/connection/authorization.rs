//! One concretely tracked connection-initialization authorization worker.

#[cfg(test)]
mod tests;

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use chigiri_http::{ConnectionInitAuthorizer, ConnectionInitDecision};
use chigiri_runtime::Object;

use super::super::request::AdmittedWebSocketRequest;
use crate::managed::EventActivity;
use crate::subscription::worker::{SubscriptionWorkerRuntimeError, TaskOwnedValue};

pub(super) type AuthorizationResult =
    Result<Option<ConnectionInitDecision>, SubscriptionWorkerRuntimeError>;

pub(super) fn authorize(
    request: Arc<AdmittedWebSocketRequest>,
    payload: Option<Arc<Object>>,
    authorizer: Arc<dyn ConnectionInitAuthorizer>,
    observer: Arc<dyn AuthorizationErrorObserver>,
    activity: EventActivity,
) -> chigiri_http::BoxFuture<'static, AuthorizationResult> {
    // These guards precede the first poll, including cancellation of a task
    // that never starts. Independent graph owners cannot unwind through each other.
    let authorizer = TaskOwnedValue::new(authorizer, Some(activity.clone()));
    let observer = TaskOwnedValue::new(observer, Some(activity.clone()));
    Box::pin(async move {
        let result = tokio::select! {
            biased;
            _ = activity.source_stopped() => return Err(SubscriptionWorkerRuntimeError::EndpointClosed),
            result = request.authorize(authorizer.as_ref(), payload.as_deref(), &activity) => result,
        };
        let decision = match result {
            Ok(decision) => Some(decision),
            Err(error) => {
                let error = TaskOwnedValue::new(error, Some(activity.clone()));
                observer.observe(&*error);
                drop(error);
                None
            }
        };
        if activity.task_failed() {
            Err(SubscriptionWorkerRuntimeError::Invariant)
        } else if activity.is_stopping() {
            Err(SubscriptionWorkerRuntimeError::EndpointClosed)
        } else {
            Ok(decision)
        }
    })
}

pub(crate) trait AuthorizationErrorObserver: Send + Sync + 'static {
    fn observe(&self, source: &(dyn Error + Send + Sync + 'static));
}

#[derive(Clone, Copy, Debug)]
pub(super) enum AuthorizationFrameworkFailure {
    Startup,
    Cancelled,
    Panicked,
    Poisoned,
    Control,
}

impl fmt::Display for AuthorizationFrameworkFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebSocket initialization failed internally")
    }
}

impl Error for AuthorizationFrameworkFailure {}
