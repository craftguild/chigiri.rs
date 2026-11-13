//! Original cleanup ticket and physical owner deferred through SDK socket disposal.

use super::{DetachedCleanup, DetachedCleanupKind};
use crate::subscription::delivery::{CleanupPhysicalCharge, CleanupTicket};

struct Resources {
    ticket: CleanupTicket,
    physical: CleanupPhysicalCharge,
}

pub(crate) struct TerminalCleanupOwner {
    resources: Option<Resources>,
    #[cfg(feature = "subscription-worker")]
    pub(super) socket_scope: Option<std::sync::Weak<crate::managed::SocketScope>>,
}

impl TerminalCleanupOwner {
    pub(in crate::subscription::bridge_resources) fn new(
        ticket: CleanupTicket,
        physical: CleanupPhysicalCharge,
    ) -> Self {
        Self {
            resources: Some(Resources { ticket, physical }),
            #[cfg(feature = "subscription-worker")]
            socket_scope: None,
        }
    }

    pub(in crate::subscription::bridge_resources) fn quarantine(
        mut self,
        cleanup: DetachedCleanup,
    ) {
        match cleanup.into_kind() {
            DetachedCleanupKind::Inline(future) => {
                let resources = self.resources.take().expect("live terminal cleanup owner");
                resources
                    .ticket
                    .quarantine_erased(future, Box::new(resources.physical));
            }
            #[cfg(feature = "subscription-worker")]
            DetachedCleanupKind::Completed => self.quarantine_completed(),
            #[cfg(feature = "subscription-worker")]
            DetachedCleanupKind::Worker {
                completion,
                retained,
            } => {
                let resources = self.resources.take().expect("live terminal cleanup owner");
                resources
                    .ticket
                    .quarantine_worker(completion, resources.physical, *retained);
            }
        }
    }

    pub(in crate::subscription::bridge_resources) fn quarantine_completed(mut self) {
        #[cfg(feature = "subscription-worker")]
        if let Some(scope) = self.socket_scope.take().and_then(|scope| scope.upgrade()) {
            scope.retain_cleanup(self);
            return;
        }
        let resources = self.resources.take().expect("live terminal cleanup owner");
        resources.ticket.quarantine_completed(resources.physical);
    }

    pub(in crate::subscription::bridge_resources) fn finish(self) {
        drop(self);
    }
}

impl Drop for TerminalCleanupOwner {
    fn drop(&mut self) {
        let Some(resources) = self.resources.take() else {
            return;
        };
        #[cfg(feature = "subscription-worker")]
        if let Some(scope) = self.socket_scope.take().and_then(|scope| scope.upgrade()) {
            // Construction can unwind before a coordinator exists. Keep this
            // original ticket and physical owner with the same SDK entry even
            // on that path; the stored owner has no back-reference to scope.
            scope.retain_cleanup(Self {
                resources: Some(resources),
                socket_scope: None,
            });
            return;
        }
        drop(resources);
    }
}
