//! The original setup budget stays outside a potentially blocked validation task.

use std::task::{Context, Poll};

use chigiri_http::{BoxFuture, DeadlineFactory};

pub(crate) struct SubscriptionSetupDeadlines<'deadline> {
    pub(super) factory: &'deadline dyn DeadlineFactory,
    pub(super) overall: BoxFuture<'deadline, ()>,
    pub(super) setup: BoxFuture<'deadline, ()>,
}

impl SubscriptionSetupDeadlines<'_> {
    /// The pending worker polls these same owners concurrently with its join.
    pub(crate) fn poll_expired(&mut self, context: &mut Context<'_>) -> Poll<()> {
        if self.overall.as_mut().poll(context).is_ready()
            || self.setup.as_mut().poll(context).is_ready()
        {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(feature = "subscription-worker")]
impl<'deadline> SubscriptionSetupDeadlines<'deadline> {
    pub(crate) const fn factory(&self) -> &'deadline dyn DeadlineFactory {
        self.factory
    }
}

#[cfg(feature = "subscription-worker")]
impl SubscriptionSetupDeadlines<'static> {
    pub(crate) fn into_monitored(
        self,
    ) -> Result<
        (
            crate::subscription::worker::MonitoredDeadlineTable,
            crate::subscription::worker::DeadlineArmer,
        ),
        crate::subscription::worker::SubscriptionWorkerRuntimeError,
    > {
        crate::subscription::worker::MonitoredDeadlineTable::with_setup(
            self.factory,
            self.overall,
            self.setup,
        )
    }

    pub(crate) fn into_initial_response_deadline(
        self,
        buffer: Option<crate::subscription::worker::InitialResponseBufferLease>,
    ) -> Result<
        crate::subscription::worker::InitialResponseDeadline,
        (
            crate::subscription::worker::SubscriptionWorkerRuntimeError,
            Option<crate::subscription::worker::InitialResponseBufferLease>,
        ),
    > {
        let Self {
            factory: _,
            overall,
            setup,
        } = self;
        drop(setup);
        let table = match crate::subscription::worker::MonitoredDeadlineTable::with_overall(overall)
        {
            Ok(table) => table,
            Err(error) => return Err((error, buffer)),
        };
        Ok(crate::subscription::worker::InitialResponseDeadline::finite(table, buffer))
    }
}
