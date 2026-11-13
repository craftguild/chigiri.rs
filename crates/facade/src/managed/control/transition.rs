//! First-notification policy, one monotonic deadline, and sticky outcomes.

use tokio::time::Instant;

use super::{Control, StopPhase};
use crate::{BoxFuture, ShutdownMode, ShutdownOptions, ShutdownOutcome};

impl Control {
    pub(crate) fn stop(&self) {
        // Owner abandonment is not another application notification. It
        // cancels even a pending drain while preserving an existing outcome.
        let mut state = self.lock();
        state.phase = StopPhase::Cancelled;
        state
            .outcome
            .get_or_insert(ShutdownOutcome::CancellationRequested);
        drop(state);
        self.notify();
    }

    pub(crate) fn begin(&self, options: ShutdownOptions, now: Instant) -> StopPhase {
        let mut state = self.lock();
        if state.phase == StopPhase::Running {
            state.phase = match options.mode() {
                ShutdownMode::Immediate => {
                    state.outcome = Some(ShutdownOutcome::CancellationRequested);
                    StopPhase::Cancelled
                }
                ShutdownMode::Graceful => {
                    match options
                        .max_duration()
                        .and_then(|duration| now.checked_add(duration))
                    {
                        Some(deadline) if state.activated || state.requests != 0 => {
                            StopPhase::Graceful { deadline }
                        }
                        Some(_) => {
                            state.outcome = Some(ShutdownOutcome::Drained);
                            StopPhase::Cancelled
                        }
                        None => {
                            state.outcome = Some(ShutdownOutcome::InfrastructureFailure);
                            StopPhase::Cancelled
                        }
                    }
                }
            };
        }
        let phase = state.phase;
        drop(state);
        self.notify();
        phase
    }

    pub(crate) fn expire(&self, now: Instant) {
        let mut state = self.lock();
        if matches!(state.phase, StopPhase::Graceful { deadline } if now >= deadline) {
            state.phase = StopPhase::Cancelled;
            state.outcome.get_or_insert(ShutdownOutcome::Expired);
            drop(state);
            self.notify();
        }
    }

    pub(crate) fn drained(&self, now: Instant) {
        let mut state = self.lock();
        if let StopPhase::Graceful { deadline } = state.phase {
            state.phase = StopPhase::Cancelled;
            state.outcome.get_or_insert(if now >= deadline {
                ShutdownOutcome::Expired
            } else {
                ShutdownOutcome::Drained
            });
        }
        drop(state);
        self.notify();
    }

    pub(crate) fn shutdown_on<F>(
        self: &std::sync::Arc<Self>,
        signal: F,
        options: ShutdownOptions,
    ) -> BoxFuture<'static, ()>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let control = std::sync::Arc::clone(self);
        Box::pin(async move {
            signal.await;
            let phase = control.begin(options, Instant::now());
            let StopPhase::Graceful { deadline } = phase else {
                return;
            };
            let mut changed = control.subscribe();
            loop {
                // Expiry wins a simultaneous completion and is never rewritten
                // by late physical join. Notification never restarts the timer.
                if Instant::now() >= deadline {
                    control.expire(Instant::now());
                    return;
                }
                if control.outcome().is_some() {
                    return;
                }
                tokio::select! {
                    biased;
                    _ = tokio::time::sleep_until(deadline) => {
                        control.expire(Instant::now());
                        return;
                    }
                    _ = changed.changed() => {}
                }
            }
        })
    }
}
