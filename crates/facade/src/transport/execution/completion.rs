//! Move-only completion belongs to execution, never to a native waiter/reaper.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use chigiri_http::{ResponseCompletion, ResponseCompletionOutcome, ResponseParts};

use crate::transport::deadline::HttpDeadlineMonitor;

pub(super) struct FiniteCompletion {
    token: Option<Arc<dyn ResponseCompletion>>,
    monitor: HttpDeadlineMonitor,
}

impl FiniteCompletion {
    pub(super) fn arm(&mut self, token: Option<Arc<dyn ResponseCompletion>>) {
        debug_assert!(self.token.is_none());
        self.token = token;
    }

    pub(super) fn timed_out(&self) {
        self.monitor.timed_out();
    }

    pub(super) fn has_token(&self) -> bool {
        self.token.is_some()
    }

    pub(super) fn new(
        token: Option<Arc<dyn ResponseCompletion>>,
        monitor: &HttpDeadlineMonitor,
    ) -> Self {
        Self {
            token,
            monitor: monitor.clone(),
        }
    }

    pub(super) fn finish(
        mut self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        let Some(token) = self.token.take() else {
            return response;
        };
        if !self.monitor.is_managed() {
            return token.complete(response, outcome);
        }
        self.invoke(token, response, outcome)
    }

    fn invoke(
        &self,
        token: Arc<dyn ResponseCompletion>,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> ResponseParts {
        // Keep the token outside the callback's unwind scope. A panicking
        // callback cannot trigger its own destructor during the same unwind.
        let result = catch_unwind(AssertUnwindSafe(|| token.complete(response, outcome)));
        let token_disposed = catch_unwind(AssertUnwindSafe(|| drop(token)));
        let mut failed = false;
        if let Err(payload) = token_disposed {
            failed = true;
            self.monitor.fail();
            if !dispose_payload(payload) {
                self.monitor.poison();
            }
        }
        match result {
            Ok(response) if !failed => response,
            Ok(response) => {
                drop(response);
                self.monitor.fail();
                empty(ResponseCompletionOutcome::Failed)
            }
            Err(payload) => {
                self.monitor.fail();
                if !dispose_payload(payload) {
                    self.monitor.poison();
                }
                self.monitor.fail();
                empty(ResponseCompletionOutcome::Failed)
            }
        }
    }
}

impl Drop for FiniteCompletion {
    fn drop(&mut self) {
        if self.token.is_none() {
            return;
        }
        let Some(outcome) = self.monitor.completion_outcome() else {
            return;
        };
        if let Some(token) = self.token.take() {
            // Future cancellation has already dropped its in-flight execution
            // child. This same concrete task attempts the token once, including
            // on executor destruction; physical join still waits for it.
            drop(self.invoke(token, empty(outcome), outcome));
        }
    }
}

fn empty(outcome: ResponseCompletionOutcome) -> ResponseParts {
    ResponseParts {
        status: if outcome == ResponseCompletionOutcome::TimedOut {
            504
        } else {
            500
        },
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn dispose_payload(payload: Box<dyn std::any::Any + Send>) -> bool {
    match catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        Ok(()) => true,
        Err(nested) => {
            if let Err(last) = catch_unwind(AssertUnwindSafe(|| drop(nested))) {
                std::mem::forget(last);
            }
            false
        }
    }
}
