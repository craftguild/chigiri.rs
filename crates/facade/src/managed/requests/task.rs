//! The only HTTP task role; no application future can be registered directly.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::BoxFuture;
use crate::managed::HttpFaultReporter;
use crate::managed::http::{HttpWork, ManagedHttpResponse};
use crate::subscription::worker::{dispose_payload, dispose_value};
use crate::transport::deadline::HttpDeadlineMonitor;
use chigiri_http::RequestBody;
use chigiri_runtime::instrumentation::InstrumentationScope;

pub(super) struct HttpTask {
    work: Option<BoxFuture<'static, ManagedHttpResponse>>,
    finished: Arc<OnceLock<Instant>>,
    monitor: HttpDeadlineMonitor,
    reporter: Option<HttpFaultReporter>,
    reported: bool,
    instrumentation: Option<InstrumentationScope>,
}

impl HttpTask {
    pub(super) fn new<C: Send + Sync + 'static, B: RequestBody + 'static>(
        work: HttpWork<C, B>,
        start: oneshot::Receiver<()>,
        finished: Arc<OnceLock<Instant>>,
        monitor: crate::transport::deadline::HttpDeadlineMonitor,
    ) -> Self {
        let reporter = work.fault_reporter();
        let instrumentation = work.handoff_scope();
        let task_monitor = monitor.clone();
        Self {
            work: Some(Box::pin(async move {
                if start.await.is_err() {
                    return ManagedHttpResponse::internal_failure();
                }
                if !monitor.registered().await {
                    return ManagedHttpResponse::internal_failure();
                }
                work.run(monitor).await
            })),
            finished,
            monitor: task_monitor,
            reporter: Some(reporter),
            reported: false,
            instrumentation,
        }
    }

    fn dispose_work(&mut self) {
        if !self.work.take().is_none_or(dispose_value) {
            self.monitor.poison();
        }
    }

    fn report_failure(&mut self) {
        if self.reported || !self.monitor.faulted() {
            return;
        }
        self.reported = true;
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            if let Some(reporter) = &self.reporter {
                reporter.report();
            }
        })) {
            self.monitor.fail();
            if !dispose_payload(payload) {
                self.monitor.poison();
            }
        }
    }
    fn poll_owned(&mut self, context: &mut Context<'_>) -> Poll<ManagedHttpResponse> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.work
                .as_mut()
                .expect("owned HTTP task")
                .as_mut()
                .poll(context)
        }));
        match result {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(response)) if !self.monitor.faulted() => {
                if matches!(
                    response,
                    ManagedHttpResponse::Finite(_) | ManagedHttpResponse::Empty { .. }
                ) && !self.monitor.admit_finite()
                {
                    return Poll::Ready(ManagedHttpResponse::unavailable());
                }
                Poll::Ready(response)
            }
            Ok(Poll::Ready(response)) => {
                drop(response);
                self.report_failure();
                Poll::Ready(ManagedHttpResponse::internal_failure())
            }
            Err(payload) => {
                self.monitor.fail();
                if !dispose_payload(payload) {
                    self.monitor.poison();
                }
                self.dispose_work();
                self.report_failure();
                Poll::Ready(ManagedHttpResponse::internal_failure())
            }
        }
    }

    fn dispose_owned(&mut self) {
        let _stamp = CompletionStamp(Arc::clone(&self.finished));
        // Timestamp only after actual application destruction, including unwind.
        self.dispose_work();
        self.report_failure();
        if !self.reporter.take().is_none_or(dispose_value) {
            self.monitor.poison();
        }
    }
}

impl Future for HttpTask {
    type Output = ManagedHttpResponse;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.instrumentation.clone() {
            Some(scope) => scope.run(|| this.poll_owned(context)),
            None => this.poll_owned(context),
        }
    }
}

struct CompletionStamp(Arc<OnceLock<Instant>>);

impl Drop for CompletionStamp {
    fn drop(&mut self) {
        let _ = self.0.set(Instant::now());
    }
}

impl Drop for HttpTask {
    fn drop(&mut self) {
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| self.dispose_owned()),
            None => self.dispose_owned(),
        }
    }
}
