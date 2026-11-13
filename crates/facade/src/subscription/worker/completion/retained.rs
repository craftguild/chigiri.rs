//! Ordered task-adjacent and physical ownership released after join readiness.

use super::dispose_value;
use crate::subscription::delivery::CleanupPhysicalCharge;
use crate::subscription::worker::InitialWorkerResources;

/// Closed physical state retained beside a worker until its join is ready.
pub(crate) enum WorkerRetainedState {
    ManagedHttp {
        _retained: Box<crate::managed::RetainedHttpRequest>,
    },
    Empty,
    Pending {
        _deadlines:
            crate::subscription::driver::setup_deadlines::SubscriptionSetupDeadlines<'static>,
        _initial: Option<InitialWorkerResources>,
    },
    Active {
        _deadlines: Option<crate::subscription::worker::MonitoredDeadlineTable>,
        _reservation: Option<crate::subscription::outbox::SubscriptionEventReservation>,
        _initial: Option<InitialWorkerResources>,
    },
    WebSocketPending {
        _deadlines: crate::subscription::worker::MonitoredDeadlineTable,
    },
    WebSocketActive {
        _deadlines: Option<crate::subscription::worker::MonitoredDeadlineTable>,
        _reservation: Option<crate::subscription::outbox::SubscriptionEventReservation>,
    },
    ConnectionInit {
        _deadline: chigiri_http::BoxFuture<'static, ()>,
    },
    NativeUpgrade {
        _retained: Box<crate::transport::websocket::RetainedWebSocketUpgrade>,
    },
}

impl WorkerRetainedState {
    pub(crate) const fn empty() -> Self {
        Self::Empty
    }

    pub(crate) const fn active(
        deadlines: crate::subscription::worker::MonitoredDeadlineTable,
        reservation: Option<crate::subscription::outbox::SubscriptionEventReservation>,
        initial: Option<InitialWorkerResources>,
    ) -> Self {
        Self::Active {
            _deadlines: Some(deadlines),
            _reservation: reservation,
            _initial: initial,
        }
    }

    pub(crate) const fn orphan_active(
        deadlines: Option<crate::subscription::worker::MonitoredDeadlineTable>,
        reservation: Option<crate::subscription::outbox::SubscriptionEventReservation>,
        initial: Option<InitialWorkerResources>,
    ) -> Self {
        Self::Active {
            _deadlines: deadlines,
            _reservation: reservation,
            _initial: initial,
        }
    }

    pub(crate) const fn pending(
        deadlines: crate::subscription::driver::setup_deadlines::SubscriptionSetupDeadlines<
            'static,
        >,
        initial: Option<InitialWorkerResources>,
    ) -> Self {
        Self::Pending {
            _deadlines: deadlines,
            _initial: initial,
        }
    }

    pub(crate) const fn websocket_pending(
        deadlines: crate::subscription::worker::MonitoredDeadlineTable,
    ) -> Self {
        Self::WebSocketPending {
            _deadlines: deadlines,
        }
    }

    pub(crate) const fn websocket_active(
        deadlines: Option<crate::subscription::worker::MonitoredDeadlineTable>,
        reservation: Option<crate::subscription::outbox::SubscriptionEventReservation>,
    ) -> Self {
        Self::WebSocketActive {
            _deadlines: deadlines,
            _reservation: reservation,
        }
    }

    pub(crate) const fn connection_init(deadline: chigiri_http::BoxFuture<'static, ()>) -> Self {
        Self::ConnectionInit {
            _deadline: deadline,
        }
    }
}

pub(crate) struct RetiredWorkerOwner {
    retained: Option<WorkerRetainedState>,
    physical: Option<CleanupPhysicalCharge>,
}

impl RetiredWorkerOwner {
    pub(super) fn is_poisoned(&self) -> bool {
        matches!(&self.retained, Some(WorkerRetainedState::ManagedHttp { _retained }) if _retained.is_poisoned())
    }
    pub(crate) const fn new(
        physical: CleanupPhysicalCharge,
        retained: WorkerRetainedState,
    ) -> Self {
        Self {
            retained: Some(retained),
            physical: Some(physical),
        }
    }

    pub(crate) const fn orphan(retained: WorkerRetainedState) -> Self {
        Self {
            retained: Some(retained),
            physical: None,
        }
    }

    pub(super) fn finish(mut self) -> bool {
        let retained = self.retained.take().expect("retired worker retained state");
        if !dispose_value(retained) {
            std::mem::forget(self.physical.take());
            return false;
        }
        self.physical.take().is_none_or(dispose_value)
    }
}

impl Drop for RetiredWorkerOwner {
    fn drop(&mut self) {
        std::mem::forget(self.retained.take());
        std::mem::forget(self.physical.take());
    }
}
