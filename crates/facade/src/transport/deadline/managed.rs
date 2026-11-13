//! SDK-owned real clocks and task-side proxies for one finite HTTP preparation.

mod terminal;
#[cfg(test)]
mod tests;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory, ResponseMediaType, ResponseParts};
use chigiri_runtime::LifecyclePhase;
use chigiri_runtime::instrumentation::{
    CapturedInstrumentation, InstrumentationKind, instrument_future,
};

use super::HttpDeadlineMonitor;
use crate::managed::{Control, ManagedServiceError};
use crate::subscription::worker::{
    DeadlineArmer, DeadlineRole, HttpPhaseDeadline, MonitoredDeadline, MonitoredDeadlineTable,
    run_monitored,
};
use crate::transport::TimeoutKind;

pub(crate) struct HttpDeadlineState {
    table: MonitoredDeadlineTable,
    deadline: tokio::time::Instant,
    terminal: Arc<terminal::Terminal>,
}

#[derive(Clone)]
pub(super) struct ManagedHttpClock {
    armer: DeadlineArmer,
    control: Arc<Control>,
    deadline: tokio::time::Instant,
    terminal: Arc<terminal::Terminal>,
}

impl HttpDeadlineState {
    pub(crate) fn finite_admitted(&self) -> bool {
        self.terminal.finite_admitted()
    }

    pub(crate) fn poisoned_flag(&self) -> &std::sync::atomic::AtomicBool {
        self.terminal.poisoned_flag()
    }
    pub(crate) fn new(
        factory: &'static dyn DeadlineFactory,
        deadline: tokio::time::Instant,
        control: Arc<Control>,
        captured: Option<CapturedInstrumentation>,
    ) -> Result<(Self, HttpDeadlineMonitor), ManagedServiceError> {
        let overall: BoxFuture<'static, ()> = Box::pin(tokio::time::sleep_until(deadline));
        let overall = match &captured {
            Some(captured) => instrument_future(
                Some(captured.handoff_scope(InstrumentationKind::Request)),
                overall,
            ),
            None => overall,
        };
        let (table, armer) = MonitoredDeadlineTable::with_request(factory, overall)
            .map_err(|_| ManagedServiceError::Startup)?;
        let armer = armer.with_instrumentation(captured);
        let terminal = Arc::new(terminal::Terminal::default());
        Ok((
            Self {
                table,
                deadline,
                terminal: Arc::clone(&terminal),
            },
            HttpDeadlineMonitor {
                managed: Some(ManagedHttpClock {
                    armer,
                    control,
                    deadline,
                    terminal,
                }),
            },
        ))
    }

    pub(crate) fn poll_timeout(
        &mut self,
        context: &mut Context<'_>,
        media: ResponseMediaType,
    ) -> Poll<Result<ResponseParts, ManagedServiceError>> {
        match self.table.poll_expired(context) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(_)) => Poll::Ready(Err(ManagedServiceError::Infrastructure)),
            Poll::Ready(Ok(fired)) => {
                self.terminal.select(true);
                let phase = fired.http_phase;
                // Select the original shorter budget even if both timers are
                // first observed after expiry. A tie retains phase precedence.
                let phase_won = phase
                    .filter(|phase| {
                        phase.cutoff <= self.deadline && tokio::time::Instant::now() >= phase.cutoff
                    })
                    .or_else(|| {
                        if fired.role == DeadlineRole::Phase
                            && tokio::time::Instant::now() < self.deadline
                        {
                            phase
                        } else {
                            None
                        }
                    });
                let response = match phase_won.map(|phase| phase.kind) {
                    Some(LifecyclePhase::BodyRead) => {
                        crate::transport::response::body_timeout_response(media)
                    }
                    Some(LifecyclePhase::OperationSourceResolution) => {
                        crate::transport::response::error_response(
                            media,
                            503,
                            "Persisted operation service is unavailable",
                            "PERSISTED_OPERATION_UNAVAILABLE",
                        )
                    }
                    _ => crate::transport::response::timeout_response(media),
                };
                Poll::Ready(Ok(response))
            }
        }
    }

    pub(crate) fn cancel(&self, timed_out: bool) {
        self.terminal.select(timed_out);
    }
}

impl ManagedHttpClock {
    pub(super) fn admit_finite(&self) -> bool {
        if self.terminal.finite_admitted() {
            return true;
        }
        let Ok(gate) = self.control.registration_gate() else {
            return false;
        };
        self.terminal.admit_finite();
        drop(gate);
        true
    }

    pub(super) fn poison(&self) {
        self.terminal.poison();
        self.control.fail();
    }
    pub(super) fn timed_out(&self) {
        if !self.terminal.faulted()
            && self.control.outcome() != Some(crate::ShutdownOutcome::InfrastructureFailure)
        {
            self.terminal.select(true);
        }
    }

    pub(super) fn completion_outcome(&self) -> chigiri_http::ResponseCompletionOutcome {
        self.terminal.outcome(
            self.control.outcome() == Some(crate::ShutdownOutcome::Expired)
                || tokio::time::Instant::now() >= self.deadline,
        )
    }

    pub(super) fn fail(&self) {
        self.terminal.fail();
        self.control.fail();
    }

    pub(super) fn faulted(&self) -> bool {
        self.terminal.faulted()
    }

    pub(super) async fn registered(&self) -> bool {
        let result = match self.armer.overall() {
            Ok(mut proxy) => {
                let result = proxy.registered().await;
                proxy.relinquish();
                result
            }
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.fail();
            return false;
        }
        true
    }

    pub(super) fn overall(&self) -> BoxFuture<'static, ()> {
        match self.armer.overall() {
            Ok(proxy) => Box::pin(Overall {
                proxy: Some(proxy),
                deadline: self.deadline,
            }),
            Err(_) => {
                self.fail();
                Box::pin(async {})
            }
        }
    }

    pub(super) async fn run<F, T>(
        &self,
        phase: LifecyclePhase,
        timeout: Duration,
        mut overall: Pin<&mut (dyn Future<Output = ()> + Send)>,
        create: impl FnOnce() -> F + Send,
    ) -> Result<T, TimeoutKind>
    where
        F: Future<Output = T> + Send,
    {
        let cutoff = tokio::time::Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| {
                self.fail();
                TimeoutKind::Overall
            })?;
        let mut timer = self
            .armer
            .arm_http_phase(
                timeout,
                HttpPhaseDeadline {
                    kind: phase,
                    cutoff,
                },
            )
            .map_err(|_| {
                self.fail();
                TimeoutKind::Overall
            })?;
        // Yield until the real timer has been externally polled. Merely waking
        // a supervisor may leave it in a blocked Tokio worker's local LIFO slot.
        // This acknowledgement arms the runtime timer before application code.
        timer.registered().await.map_err(|_| {
            self.fail();
            TimeoutKind::Overall
        })?;
        if tokio::time::Instant::now() >= cutoff {
            timer.relinquish();
            return Err(TimeoutKind::Phase);
        }
        let result = run_monitored(&mut timer, &mut overall, create()).await;
        if result.is_ok() && tokio::time::Instant::now() >= cutoff {
            timer.relinquish();
            return Err(TimeoutKind::Phase);
        }
        if result.is_err() {
            timer.relinquish();
            return result;
        }
        if timer.clear().is_err() {
            self.fail();
            return Err(TimeoutKind::Overall);
        }
        result
    }
}

struct Overall {
    proxy: Option<MonitoredDeadline>,
    deadline: tokio::time::Instant,
}

impl Future for Overall {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if tokio::time::Instant::now() >= self.deadline {
            return Poll::Ready(());
        }
        Pin::new(self.proxy.as_mut().expect("HTTP overall proxy")).poll(context)
    }
}

impl Drop for Overall {
    fn drop(&mut self) {
        // The request's exact clock survives nested subscription handoff and
        // blocking destruction until its original concrete HTTP join completes.
        if let Some(proxy) = self.proxy.take() {
            proxy.relinquish();
        }
    }
}
