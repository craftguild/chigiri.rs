//! Shared source initializer timing below protocol-specific response handling.

use std::time::Duration;

use chigiri_runtime::__private::{PreparedSubscription, SubscriptionStart};

use crate::subscription::worker::{
    DeadlineArmer, DeadlineRole, MonitoredDeadline, SubscriptionWorkerRuntimeError, run_monitored,
};

pub(crate) enum MonitoredSourceStart<C> {
    Completed {
        started: SubscriptionStart<C>,
        deadline: MonitoredDeadline,
    },
    TimedOut,
}

pub(crate) async fn start_source_monitored<C, I>(
    prepared: PreparedSubscription<C>,
    context: &C,
    execution_timeout: Duration,
    armer: &DeadlineArmer,
    inherited: &mut I,
) -> Result<MonitoredSourceStart<C>, SubscriptionWorkerRuntimeError>
where
    C: Send + Sync + 'static,
    I: std::future::Future<Output = ()> + Send + Unpin,
{
    let mut deadline = armer.arm(DeadlineRole::Phase, execution_timeout)?;
    match run_monitored(
        &mut deadline,
        inherited,
        prepared.start(context, Some(armer.factory())),
    )
    .await
    {
        Ok(started) => Ok(MonitoredSourceStart::Completed { started, deadline }),
        Err(_) => Ok(MonitoredSourceStart::TimedOut),
    }
}
