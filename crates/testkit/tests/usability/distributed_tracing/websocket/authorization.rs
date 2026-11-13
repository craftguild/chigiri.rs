//! Real authorizer method, polls, error observer and destruction inspect control.

use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};

use opentelemetry::Context;
use opentelemetry::trace::{SpanContext, TraceContextExt};

use super::super::PrivateValue;

pub(super) struct Check {
    pub(super) selected: SpanContext,
    pub(super) suppressed: bool,
    pub(super) stages: Mutex<Vec<&'static str>>,
}
impl Check {
    pub(super) fn record(&self, stage: &'static str) {
        let current = Context::current();
        assert_eq!(*current.span().span_context(), self.selected, "{stage}");
        assert_eq!(
            current.is_telemetry_suppressed(),
            self.suppressed,
            "{stage}"
        );
        assert!(current.get::<PrivateValue>().is_none());
        self.stages.lock().unwrap().push(stage);
    }
}

struct FutureDrop(Arc<Check>);
impl Drop for FutureDrop {
    fn drop(&mut self) {
        self.0.record("future-drop");
    }
}

struct Failure(Arc<Check>);
impl fmt::Debug for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("private authorization failure")
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("private authorization failure")
    }
}
impl Error for Failure {}
impl Drop for Failure {
    fn drop(&mut self) {
        self.0.record("source-drop");
    }
}

pub(super) struct Authorize {
    pub(super) check: Arc<Check>,
    pub(super) fail: bool,
}
impl chigiri_http::ConnectionInitAuthorizer for Authorize {
    fn authorize<'a>(
        &'a self,
        _: chigiri_http::ConnectionInitContext<'a>,
    ) -> chigiri::BoxFuture<
        'a,
        Result<chigiri_http::ConnectionInitDecision, chigiri_http::ConnectionInitError>,
    > {
        self.check.record("construct");
        let probe = FutureDrop(Arc::clone(&self.check));
        Box::pin(async move {
            let _probe = probe;
            self.check.record("poll");
            tokio::task::yield_now().await;
            self.check.record("resume");
            if self.fail {
                Err(chigiri_http::ConnectionInitError::new(Failure(Arc::clone(
                    &self.check,
                ))))
            } else {
                Ok(chigiri_http::ConnectionInitDecision::Accept)
            }
        })
    }
}

pub(super) struct Errors(pub(super) Arc<Check>);
impl chigiri::ErrorObserver for Errors {
    fn observe_error(&self, event: chigiri::ErrorEvent<'_>) {
        assert_eq!(event.code(), "INTERNAL_SERVER_ERROR");
        self.0.record("error-observer");
    }
}
