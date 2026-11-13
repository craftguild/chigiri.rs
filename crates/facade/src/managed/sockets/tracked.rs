//! Every entry finishes application disposal before its physical scope releases.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::{Context, Poll};

use super::{SocketScope, SocketWork};
use crate::managed::{HttpFaultReporter, ManagedServiceError, StopPhase};
use crate::subscription::worker::{dispose_payload, dispose_value};
use chigiri_runtime::instrumentation::InstrumentationScope;

pub(super) struct TrackedWork {
    work: Option<Box<dyn SocketWork>>,
    scope: Arc<SocketScope>,
    reporter: Option<HttpFaultReporter>,
    instrumentation: Option<InstrumentationScope>,
}
impl TrackedWork {
    pub(super) fn new(
        work: Box<dyn SocketWork>,
        scope: Arc<SocketScope>,
        reporter: HttpFaultReporter,
        instrumentation: Option<InstrumentationScope>,
    ) -> Self {
        Self {
            work: Some(work),
            scope,
            reporter: Some(reporter),
            instrumentation,
        }
    }
    fn report(&self) {
        if !self.scope.failed() {
            return;
        }
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            if let Some(reporter) = &self.reporter {
                reporter.report();
            }
        })) {
            self.scope.fail();
            if !dispose_payload(payload) {
                self.scope.poison();
            }
        }
    }

    fn poll_owned(
        &mut self,
        context: &mut Context<'_>,
        phase: StopPhase,
    ) -> Poll<Result<(), ManagedServiceError>> {
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.work
                .as_mut()
                .expect("live socket entry")
                .poll(context, phase)
        }));
        match result {
            Ok(result) => result,
            Err(payload) => {
                self.scope.fail();
                if !dispose_payload(payload) {
                    self.scope.poison();
                }
                Poll::Ready(Err(ManagedServiceError::Infrastructure))
            }
        }
    }
}
impl TrackedWork {
    fn dispose_owned(&mut self) {
        if !self.work.take().is_none_or(dispose_value) {
            self.scope.poison();
        }
        self.report();
        if !self.reporter.take().is_none_or(dispose_value) {
            self.scope.poison();
        }
        self.scope.finish();
    }
}

impl SocketWork for TrackedWork {
    fn poll(
        &mut self,
        context: &mut Context<'_>,
        phase: StopPhase,
    ) -> Poll<Result<(), ManagedServiceError>> {
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| self.poll_owned(context, phase)),
            None => self.poll_owned(context, phase),
        }
    }
}

impl Drop for TrackedWork {
    fn drop(&mut self) {
        match self.instrumentation.clone() {
            Some(scope) => scope.run(|| self.dispose_owned()),
            None => self.dispose_owned(),
        }
    }
}
