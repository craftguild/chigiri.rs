//! Track all accepted jobs so request completion does not depend on a reply receiver's lifetime.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::Notify;

use super::Client;

#[derive(Default)]
pub(super) struct Pending {
    state: Mutex<State>,
    changed: Notify,
}

#[derive(Default)]
struct State {
    count: usize,
    terminal: Option<Box<dyn FnOnce() + Send>>,
}

pub(super) struct Ticket {
    pending: Arc<Pending>,
    request: Option<Arc<super::request::Request>>,
    accepted: bool,
    completed: bool,
    pub diagnostics: Option<Arc<dyn Fn(crate::luau::ScriptError) + Send + Sync>>,
}

impl Ticket {
    pub fn report(&self, error: &crate::luau::ScriptError) {
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics(error.clone());
        }
    }

    pub fn accept(&mut self) {
        self.accepted = true;
    }

    pub fn complete(&mut self) {
        self.completed = true;
    }
}

impl Pending {
    pub fn ticket(self: &Arc<Self>, request: Option<Arc<super::request::Request>>) -> Ticket {
        self.state.lock().expect("pending-job state lock").count += 1;
        Ticket {
            pending: self.clone(),
            request,
            accepted: false,
            completed: false,
            diagnostics: None,
        }
    }

    fn terminal(&self, terminal: impl FnOnce() + Send + 'static) {
        let mut state = self.state.lock().expect("pending-job state lock");
        if state.count != 0 {
            debug_assert!(state.terminal.is_none());
            state.terminal = Some(Box::new(terminal));
            return;
        }
        drop(state);
        terminal();
    }

    async fn wait(&self, deadline: Instant) -> bool {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // Register before checking state so final job closure cannot be missed.
            changed.as_mut().enable();
            if self.state.lock().expect("pending-job state lock").count == 0 {
                return true;
            }
            if tokio::time::timeout_at(deadline.into(), changed)
                .await
                .is_err()
            {
                return false;
            }
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        // Only an accepted job that failed to deliver a reply becomes an independent failure.
        if self.accepted
            && !self.completed
            && let Some(request) = &self.request
        {
            request.fail_reply();
            self.report(&super::reply_missing());
        }
        let mut state = self.pending.state.lock().expect("pending-job state lock");
        state.count -= 1;
        if state.count == 0 {
            let terminal = state.terminal.take();
            drop(state);
            self.pending.changed.notify_waiters();
            if let Some(terminal) = terminal {
                terminal();
            }
        }
    }
}

impl Client {
    /// Install a nonblocking diagnostic sink shared by the job and reply receiver.
    pub(in crate::service) fn with_diagnostics(
        &self,
        diagnostics: impl Fn(crate::luau::ScriptError) + Send + Sync + 'static,
    ) -> Self {
        let mut client = self.clone();
        client.diagnostics = Some(Arc::new(diagnostics));
        client
    }

    #[cfg(test)]
    pub(in crate::service) fn pending_job(&self) -> impl Drop + use<> {
        self.request.as_ref().unwrap().pending.ticket(None)
    }

    /// Run terminal diagnostics after all jobs close without retaining the request session.
    pub(in crate::service) fn terminal_after_jobs(&self, terminal: impl FnOnce() + Send + 'static) {
        match &self.request {
            Some(request) => request.pending.terminal(terminal),
            None => terminal(),
        }
    }

    /// Wait until every request job closes, bounded by the request deadline.
    pub(in crate::service) async fn settle_request(&self, deadline: Instant) -> bool {
        match &self.request {
            Some(request) => request.pending.wait(deadline).await,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    #[tokio::test]
    async fn an_undelivered_job_reports_the_same_failure_to_diagnostics_and_receiver() {
        let ir = crate::service::tests::ir("type Query{value:Int! @chigiri_mock_resolver}");
        let (base, receiver) = super::super::queue(ir);
        let execution = crate::runtime::Execution {
            state: base.initial_state().snapshot(),
            request: None,
            deadline: Instant::now() + crate::limits::OPERATION_TIME,
            cancel: Default::default(),
            shutdown: base.shutdown.clone(),
            budget: Default::default(),
        };
        let errors = Arc::new(Mutex::new(Vec::new()));
        let diagnostics = errors.clone();
        let client = base
            .request_client(false, &execution)
            .await
            .unwrap()
            .with_diagnostics(move |error| diagnostics.lock().unwrap().push(error));
        let graph = client.ir().graph();
        let field = graph.types()[graph.query().0].fields["value"];
        let mut reply =
            Box::pin(client.execute_with(field, Default::default(), None, None, execution));
        std::future::poll_fn(|cx| {
            assert!(reply.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(receiver.try_recv().unwrap());
        let received = reply.await.err().unwrap();
        let errors = errors.lock().unwrap();
        assert_eq!(errors.len(), 1);
        let worker = &errors[0];
        assert_eq!(
            (worker.phase, worker.body, worker.message),
            (received.phase, received.body, received.message)
        );
        assert!(!worker.cancelled && !received.cancelled);
    }

    #[test]
    fn terminal_callback_waits_for_registration_and_the_last_job_in_either_order() {
        for finish_first in [false, true] {
            let pending = Arc::new(Pending::default());
            let ticket = pending.ticket(None);
            let mut held = Some(ticket);
            if finish_first {
                drop(held.take());
            }
            let weak = Arc::downgrade(&pending);
            let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let recorded = calls.clone();
            pending.terminal(move || {
                let pending = weak.upgrade().unwrap();
                assert_eq!(pending.state.try_lock().unwrap().count, 0);
                recorded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
            drop(held);
            assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        }
    }

    #[tokio::test]
    async fn request_does_not_settle_until_its_job_is_dropped() {
        let ir = crate::service::tests::ir("type Query{value:Int! @chigiri_mock_resolver}");
        let (base, receiver) = super::super::queue(ir);
        let execution = crate::runtime::Execution {
            state: base.initial_state().snapshot(),
            request: None,
            deadline: Instant::now() + crate::limits::OPERATION_TIME,
            cancel: Default::default(),
            shutdown: base.shutdown.clone(),
            budget: Default::default(),
        };
        let client = base.request_client(false, &execution).await.unwrap();
        let graph = client.ir().graph();
        let field = graph.types()[graph.query().0].fields["value"];
        let mut reply =
            Box::pin(client.execute_with(field, Default::default(), None, None, execution.clone()));
        std::future::poll_fn(|cx| {
            assert!(reply.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let job = receiver.try_recv().unwrap();
        drop(reply);
        assert!(job.cancel.cancelled());
        let mut settled = Box::pin(client.settle_request(execution.deadline));
        std::future::poll_fn(|cx| {
            assert!(settled.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        // Dropping the reply future does not let the request settle before its worker job closes.
        drop(job);
        assert!(settled.await);
        assert_eq!(
            execution.budget.failure(),
            Some(crate::limits::request::BudgetFailure::Execution)
        );
        assert!(client.commit_request().is_err());
    }

    #[tokio::test]
    async fn unfinished_job_does_not_settle_before_the_requested_deadline() {
        let pending = Arc::new(Pending::default());
        let ticket = pending.ticket(None);
        assert!(!pending.wait(Instant::now()).await);
        drop(ticket);
        assert!(pending.wait(Instant::now()).await);
    }
}
