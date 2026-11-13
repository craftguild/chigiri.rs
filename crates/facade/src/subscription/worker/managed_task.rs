//! Closed managed subscription task owns application polling and destruction.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::BoxFuture;
use crate::managed::{EventActivity, HttpFaultReporter};
use crate::subscription::worker::{SubscriptionWorkerRuntimeError, dispose_payload, dispose_value};
use chigiri_runtime::instrumentation::InstrumentationScope;

/// Records managed infrastructure failure while its short instrumentation is active.
/// The outer task still owns reporting, typed completion and physical evidence.
pub(crate) async fn guard_instrumented_work<F, T>(
    work: F,
    activity: &EventActivity,
) -> Result<T, SubscriptionWorkerRuntimeError>
where
    F: Future<Output = Result<T, SubscriptionWorkerRuntimeError>> + Send,
{
    let mut work = super::TaskOwnedFuture::new(work, Some(activity.clone()));
    std::future::poll_fn(|context| {
        match catch_unwind(AssertUnwindSafe(|| Pin::new(&mut work).poll(context))) {
            Ok(Poll::Ready(Err(
                error @ (SubscriptionWorkerRuntimeError::Invariant
                | SubscriptionWorkerRuntimeError::Poisoned),
            ))) => {
                activity.fail_task();
                Poll::Ready(Err(error))
            }
            Ok(polled) => polled,
            Err(payload) => {
                activity.fail_task();
                if !dispose_payload(payload) {
                    activity.poison_task();
                }
                Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant))
            }
        }
    })
    .await
}

/// Native output roles contain no application-owned context, source or error.
pub(super) trait ClosedTaskOutput: Send + 'static {}
impl ClosedTaskOutput for () {}
impl ClosedTaskOutput for chigiri_runtime::GraphQlResponse {}
impl ClosedTaskOutput for Option<chigiri_http::ConnectionInitDecision> {}

pub(crate) fn guard_authorization(
    work: impl Future<
        Output = Result<
            Option<chigiri_http::ConnectionInitDecision>,
            SubscriptionWorkerRuntimeError,
        >,
    > + Send
    + 'static,
    activity: EventActivity,
    reporter: HttpFaultReporter,
    instrumentation: Option<InstrumentationScope>,
) -> BoxFuture<
    'static,
    Result<Option<chigiri_http::ConnectionInitDecision>, SubscriptionWorkerRuntimeError>,
> {
    Box::pin(
        ManagedSubscriptionTask::new(work, activity, reporter)
            .with_instrumentation(instrumentation),
    )
}

pub(super) struct ManagedSubscriptionTask<T: ClosedTaskOutput> {
    work: Option<BoxFuture<'static, Result<T, SubscriptionWorkerRuntimeError>>>,
    activity: EventActivity,
    reporter: Option<HttpFaultReporter>,
    reported: bool,
    instrumentation: Option<InstrumentationScope>,
}

impl<T: ClosedTaskOutput> ManagedSubscriptionTask<T> {
    pub(super) fn new(
        work: impl Future<Output = Result<T, SubscriptionWorkerRuntimeError>> + Send + 'static,
        activity: EventActivity,
        reporter: HttpFaultReporter,
    ) -> Self {
        Self {
            work: Some(Box::pin(work)),
            activity,
            reporter: Some(reporter),
            reported: false,
            instrumentation: None,
        }
    }

    pub(super) fn with_instrumentation(mut self, scope: Option<InstrumentationScope>) -> Self {
        self.instrumentation = scope;
        self
    }

    fn dispose_work(&mut self) {
        if !self.work.take().is_none_or(dispose_value) {
            self.activity.poison_task();
        }
    }

    fn report_failure(&mut self) {
        if self.reported || !self.activity.task_failed() {
            return;
        }
        self.reported = true;
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            if let Some(reporter) = &self.reporter {
                reporter.report();
            }
        })) {
            self.activity.fail_task();
            if !dispose_payload(payload) {
                self.activity.poison_task();
            }
        }
    }
    fn poll_owned(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<T, SubscriptionWorkerRuntimeError>> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.work
                .as_mut()
                .expect("live managed subscription task")
                .as_mut()
                .poll(context)
        }));
        match result {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(result)) => {
                if matches!(
                    result,
                    Err(SubscriptionWorkerRuntimeError::Invariant
                        | SubscriptionWorkerRuntimeError::Poisoned)
                ) {
                    self.activity.fail_task();
                }
                self.dispose_work();
                self.report_failure();
                if self.activity.task_failed() {
                    if let Ok(value) = result
                        && !dispose_value(value)
                    {
                        self.activity.poison_task();
                    }
                    Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant))
                } else {
                    Poll::Ready(result)
                }
            }
            Err(payload) => {
                self.activity.fail_task();
                if !dispose_payload(payload) {
                    self.activity.poison_task();
                }
                self.dispose_work();
                self.report_failure();
                Poll::Ready(Err(SubscriptionWorkerRuntimeError::Invariant))
            }
        }
    }

    fn dispose_owned(&mut self) {
        self.dispose_work();
        self.report_failure();
        if !self.reporter.take().is_none_or(dispose_value) {
            self.activity.poison_task();
        }
    }
}

impl<T: ClosedTaskOutput> Future for ManagedSubscriptionTask<T> {
    type Output = Result<T, SubscriptionWorkerRuntimeError>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.instrumentation.clone() {
            Some(scope) => scope.run(|| this.poll_owned(context)),
            None => this.poll_owned(context),
        }
    }
}

impl<T: ClosedTaskOutput> Drop for ManagedSubscriptionTask<T> {
    fn drop(&mut self) {
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| self.dispose_owned()),
            None => self.dispose_owned(),
        }
    }
}
