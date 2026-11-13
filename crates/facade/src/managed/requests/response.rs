//! Native response waiting does not drive execution, clocks or concrete joins.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::oneshot;

use super::Disconnect;
use crate::managed::{Control, ManagedHttpResponse};
use crate::{BoxFuture, ShutdownOutcome};

pub(super) struct RequestResponse {
    wait: BoxFuture<'static, ManagedHttpResponse>,
    disconnect: Arc<Disconnect>,
    consumed: bool,
    control: Arc<Control>,
}

impl RequestResponse {
    pub(super) fn new(
        mut receiver: oneshot::Receiver<ManagedHttpResponse>,
        disconnect: Arc<Disconnect>,
        control: Arc<Control>,
    ) -> Self {
        let waiting = Arc::clone(&control);
        let wait = Box::pin(async move {
            let control = waiting;
            tokio::select! {
                biased;
                _ = control.cancelled() => {
                    // Drained means an eligible finite response was prepared
                    // and physically joined before the native stop hook ended.
                    if control.outcome() == Some(ShutdownOutcome::Drained) {
                        receiver.await.unwrap_or_else(|_| ManagedHttpResponse::internal_failure())
                    } else {
                        cancelled(&control)
                    }
                }
                response = &mut receiver => response.unwrap_or_else(|_| {
                    control.fail();
                    ManagedHttpResponse::internal_failure()
                }),
            }
        });
        Self {
            wait,
            disconnect,
            consumed: false,
            control,
        }
    }
}

impl Future for RequestResponse {
    type Output = ManagedHttpResponse;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Poll::Ready(response) = self.wait.as_mut().poll(context) else {
            return Poll::Pending;
        };
        let allowed = match self.control.emission_gate() {
            Ok(gate) => {
                drop(gate);
                true
            }
            Err(_) => self.control.outcome() == Some(ShutdownOutcome::Drained),
        };
        self.consumed = true;
        // Authorization is ordered with shutdown, but destruction and native
        // conversion never execute while the endpoint admission gate is held.
        if allowed {
            Poll::Ready(response)
        } else {
            drop(response);
            Poll::Ready(cancelled(&self.control))
        }
    }
}

impl Drop for RequestResponse {
    fn drop(&mut self) {
        if !self.consumed {
            self.disconnect.close();
        }
    }
}

pub(super) fn cancelled(control: &Control) -> ManagedHttpResponse {
    if control.outcome() == Some(ShutdownOutcome::InfrastructureFailure) {
        ManagedHttpResponse::internal_failure()
    } else {
        ManagedHttpResponse::unavailable()
    }
}
