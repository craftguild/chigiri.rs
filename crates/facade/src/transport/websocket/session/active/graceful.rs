//! Common shutdown policy; native adapters retain only socket conversion.

use std::sync::Weak;

use super::ActiveConnection;
use crate::managed::StopPhase;
use crate::transport::websocket::session::WebSocketIo;

pub(super) enum SendAuthorization {
    Allowed,
    ReconcileGraceful,
    Stopped,
}

impl<C: Send + Sync + 'static, S: WebSocketIo> ActiveConnection<C, S> {
    /// Linearizes a whole-frame permission after all fallible preparation.
    /// Release the gate before native I/O, callbacks, wakes or owner disposal.
    pub(super) fn authorize_send(&self, finite: bool) -> SendAuthorization {
        let Some(managed) = self.configuration.execution.managed.get() else {
            return SendAuthorization::Allowed;
        };
        let Some(control) = managed.upgrade() else {
            return SendAuthorization::Stopped;
        };
        let Ok(gate) = control.emission_gate() else {
            return SendAuthorization::Stopped;
        };
        match gate.phase() {
            StopPhase::Running => SendAuthorization::Allowed,
            StopPhase::Graceful { .. } if self.draining || finite => SendAuthorization::Allowed,
            StopPhase::Graceful { .. } => SendAuthorization::ReconcileGraceful,
            StopPhase::Cancelled => SendAuthorization::Stopped,
        }
    }

    pub(super) fn reconcile_send(&mut self, authorization: SendAuthorization) {
        match authorization {
            SendAuthorization::Allowed => {}
            SendAuthorization::ReconcileGraceful => self.graceful(),
            SendAuthorization::Stopped => self.shutdown(),
        }
    }

    pub(in crate::transport::websocket::session) fn graceful(&mut self) {
        if self.draining || self.closing.is_some() {
            return;
        }
        self.draining = true;
        self.heartbeat_due = false;
        if let Some(parts) = &mut self.parts {
            parts.controls.discard_waiting();
            for slot in self.slots.iter_mut().flatten() {
                if slot.begin_graceful(&mut parts.controls).is_err() {
                    self.close_internal();
                    return;
                }
            }
        }
    }

    pub(super) fn sync_shutdown(&mut self) {
        let Some(managed) = self.configuration.execution.managed.get() else {
            return;
        };
        let Some(control) = Weak::upgrade(managed) else {
            self.shutdown();
            return;
        };
        control.expire(tokio::time::Instant::now());
        match control.phase() {
            StopPhase::Running => {}
            StopPhase::Graceful { .. } => self.graceful(),
            StopPhase::Cancelled => self.shutdown(),
        }
    }

    pub(super) fn finish_graceful(&mut self) {
        if self.draining
            && self.current.is_none()
            && self.slots.iter().all(Option::is_none)
            && self
                .parts
                .as_ref()
                .is_some_and(|parts| parts.controls.queued_messages() == 0)
        {
            self.shutdown();
        }
    }
}
