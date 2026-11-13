//! The single original WebSocket operation setup deadline across both workers.

use chigiri_http::{BoxFuture, DeadlineFactory};

use crate::subscription::worker::{
    DeadlineArmer, MonitoredDeadlineTable, SubscriptionWorkerRuntimeError,
};

pub(crate) struct WebSocketSetupDeadline {
    factory: &'static dyn DeadlineFactory,
    setup: BoxFuture<'static, ()>,
}

pub(super) struct MonitoredWebSocketDeadlines {
    factory: &'static dyn DeadlineFactory,
    table: MonitoredDeadlineTable,
    armer: DeadlineArmer,
}

impl WebSocketSetupDeadline {
    pub(crate) fn new(
        factory: &'static dyn DeadlineFactory,
        setup: BoxFuture<'static, ()>,
    ) -> Self {
        Self { factory, setup }
    }

    pub(super) const fn factory(&self) -> &'static dyn DeadlineFactory {
        self.factory
    }

    pub(super) fn into_monitored(
        self,
    ) -> Result<MonitoredWebSocketDeadlines, SubscriptionWorkerRuntimeError> {
        let (table, armer) = MonitoredDeadlineTable::with_setup_only(self.factory, self.setup)?;
        Ok(MonitoredWebSocketDeadlines {
            factory: self.factory,
            table,
            armer,
        })
    }
}

impl MonitoredWebSocketDeadlines {
    pub(super) const fn factory(&self) -> &'static dyn DeadlineFactory {
        self.factory
    }

    pub(super) fn armer(&self) -> DeadlineArmer {
        self.armer.clone()
    }

    pub(super) fn poll_expired(
        &mut self,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), SubscriptionWorkerRuntimeError>> {
        self.table
            .poll_expired(context)
            .map(|result| result.map(|_| ()))
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        MonitoredDeadlineTable,
        DeadlineArmer,
        &'static dyn DeadlineFactory,
    ) {
        (self.table, self.armer, self.factory)
    }

    pub(super) fn into_table(self) -> MonitoredDeadlineTable {
        self.table
    }
}
