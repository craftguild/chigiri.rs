//! Externally polled single-owner deadline futures for application-call phases.

mod proxy;
mod state;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use chigiri_http::{BoxFuture, DeadlineFactory};
use chigiri_runtime::instrumentation::{
    CapturedInstrumentation, InstrumentationKind, instrument_future,
};

pub(crate) use proxy::{MonitoredDeadline, MonitoredDeadlinePair};
use state::DeadlineState;

use super::error::SubscriptionWorkerRuntimeError;
use crate::transport::TimeoutKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeadlineRole {
    Overall,
    Setup,
    Event,
    Phase,
}

impl DeadlineRole {
    pub(super) const fn index(self) -> usize {
        match self {
            Self::Overall => 0,
            Self::Setup => 1,
            Self::Event => 2,
            Self::Phase => 3,
        }
    }

    pub(super) const fn all() -> [Self; 4] {
        [Self::Overall, Self::Setup, Self::Event, Self::Phase]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FiredDeadline {
    pub(crate) role: DeadlineRole,
    pub(crate) generation: u64,
    pub(crate) http_phase: Option<HttpPhaseDeadline>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HttpPhaseDeadline {
    pub(crate) kind: chigiri_runtime::LifecyclePhase,
    pub(crate) cutoff: tokio::time::Instant,
}

/// Sole external owner and polling path for every exact timer future.
pub(crate) struct MonitoredDeadlineTable {
    state: Arc<DeadlineState>,
}

impl MonitoredDeadlineTable {
    pub(crate) fn with_request(
        factory: &'static dyn DeadlineFactory,
        overall: BoxFuture<'static, ()>,
    ) -> Result<(Self, DeadlineArmer), SubscriptionWorkerRuntimeError> {
        let state = DeadlineState::with_overall(overall)?;
        Ok((
            Self {
                state: Arc::clone(&state),
            },
            DeadlineArmer {
                state,
                factory,
                activity: None,
                instrumentation: None,
            },
        ))
    }

    pub(crate) fn with_overall(
        overall: BoxFuture<'static, ()>,
    ) -> Result<Self, SubscriptionWorkerRuntimeError> {
        Ok(Self {
            state: DeadlineState::with_overall(overall)?,
        })
    }

    pub(crate) fn with_setup(
        factory: &'static dyn DeadlineFactory,
        overall: BoxFuture<'static, ()>,
        setup: BoxFuture<'static, ()>,
    ) -> Result<(Self, DeadlineArmer), SubscriptionWorkerRuntimeError> {
        let state = DeadlineState::with_setup(overall, setup)?;
        Ok((
            Self {
                state: Arc::clone(&state),
            },
            DeadlineArmer {
                state,
                factory,
                activity: None,
                instrumentation: None,
            },
        ))
    }

    pub(crate) fn with_setup_only(
        factory: &'static dyn DeadlineFactory,
        setup: BoxFuture<'static, ()>,
    ) -> Result<(Self, DeadlineArmer), SubscriptionWorkerRuntimeError> {
        let state = DeadlineState::with_setup_only(setup)?;
        Ok((
            Self {
                state: Arc::clone(&state),
            },
            DeadlineArmer {
                state,
                factory,
                activity: None,
                instrumentation: None,
            },
        ))
    }

    pub(crate) fn poll_expired(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Result<FiredDeadline, SubscriptionWorkerRuntimeError>> {
        self.state.poll_expired(context)
    }

    pub(crate) fn close(&self) {
        self.state.close();
    }

    pub(crate) fn commit_initial_response(&self) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.state.clear_current(DeadlineRole::Overall)
    }

    pub(crate) fn finish_setup(&self) -> Result<(), SubscriptionWorkerRuntimeError> {
        self.state.clear_current(DeadlineRole::Setup)
    }
}

impl Drop for MonitoredDeadlineTable {
    fn drop(&mut self) {
        self.state.close();
    }
}

/// Task-side capability that installs an exact timer before application code.
#[derive(Clone)]
pub(crate) struct DeadlineArmer {
    state: Arc<DeadlineState>,
    factory: &'static dyn DeadlineFactory,
    activity: Option<crate::managed::EventActivity>,
    instrumentation: Option<CapturedInstrumentation>,
}

impl DeadlineArmer {
    pub(crate) fn with_instrumentation(
        mut self,
        captured: Option<CapturedInstrumentation>,
    ) -> Self {
        self.instrumentation = captured;
        self
    }

    fn timer(&self, duration: Duration) -> BoxFuture<'static, ()> {
        let Some(captured) = &self.instrumentation else {
            return self.factory.sleep(duration);
        };
        let scope = captured.child_handoff_scope(InstrumentationKind::Provider {
            phase: chigiri_runtime::LifecyclePhase::Execution,
            component: "deadline",
        });
        let timer = scope.run(|| self.factory.sleep(duration));
        instrument_future(Some(scope), timer)
    }

    pub(crate) fn with_task(mut self, activity: crate::managed::EventActivity) -> Self {
        self.activity = Some(activity);
        self
    }

    fn scoped(
        &self,
        deadline: Result<MonitoredDeadline, SubscriptionWorkerRuntimeError>,
    ) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        deadline.map(|deadline| deadline.with_task(self.activity.clone()))
    }

    pub(crate) fn overall(&self) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        self.scoped(self.state.proxy(DeadlineRole::Overall))
    }

    pub(crate) const fn factory(&self) -> &'static dyn DeadlineFactory {
        self.factory
    }

    pub(crate) fn arm(
        &self,
        role: DeadlineRole,
        duration: Duration,
    ) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        let timer = self.timer(duration);
        self.scoped(self.state.arm(role, timer))
    }

    pub(crate) fn arm_http_phase(
        &self,
        duration: Duration,
        phase: HttpPhaseDeadline,
    ) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        self.scoped(self.state.arm_http_phase(self.timer(duration), phase))
    }

    pub(crate) fn setup_pair(
        &self,
    ) -> Result<MonitoredDeadlinePair, SubscriptionWorkerRuntimeError> {
        let overall = self.overall()?;
        let setup = self.setup()?;
        Ok(MonitoredDeadlinePair::new(overall, setup))
    }

    pub(crate) fn setup(&self) -> Result<MonitoredDeadline, SubscriptionWorkerRuntimeError> {
        self.scoped(self.state.proxy(DeadlineRole::Setup))
    }
}

pub(crate) async fn run_monitored<F, I, T>(
    phase: &mut MonitoredDeadline,
    inherited: &mut I,
    future: F,
) -> Result<T, TimeoutKind>
where
    F: std::future::Future<Output = T> + Send,
    I: std::future::Future<Output = ()> + Send + Unpin,
{
    use super::task_ownership::{TaskOwnedFuture, TaskOwnedValue};
    let activity = phase.task_activity();
    let mut future = TaskOwnedFuture::new(future, activity.clone());
    std::future::poll_fn(|context| {
        if std::pin::Pin::new(&mut *phase).poll(context).is_ready() {
            return Poll::Ready(Err(TimeoutKind::Phase));
        }
        if std::pin::Pin::new(&mut *inherited).poll(context).is_ready() {
            return Poll::Ready(Err(TimeoutKind::Overall));
        }
        let value = match std::pin::Pin::new(&mut future).poll(context) {
            Poll::Ready(value) => value,
            Poll::Pending => return Poll::Pending,
        };
        let value = TaskOwnedValue::new(value, activity.clone());
        if std::pin::Pin::new(&mut *phase).poll(context).is_ready() {
            return Poll::Ready(Err(TimeoutKind::Phase));
        }
        if std::pin::Pin::new(&mut *inherited).poll(context).is_ready() {
            return Poll::Ready(Err(TimeoutKind::Overall));
        }
        Poll::Ready(Ok(value.into_inner()))
    })
    .await
}
