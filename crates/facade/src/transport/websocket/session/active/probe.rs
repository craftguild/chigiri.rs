//! Synchronization points for concrete same-socket driver fixtures.

use super::{ActiveConnection, WebSocketIo};
use crate::subscription::bridge_resources::BridgeGeneration;
use crate::subscription::control::WebSocketControlClass;

impl<C: Send + Sync + 'static, S: WebSocketIo> ActiveConnection<C, S> {
    pub(in crate::transport::websocket::session) fn setup_finished_for_test(
        &self,
        id: &str,
    ) -> bool {
        self.slots
            .iter()
            .flatten()
            .find(|slot| slot.wire_id() == Some(id))
            .is_some_and(|slot| slot.setup_finished_for_test())
    }

    pub(in crate::transport::websocket::session) fn bind_managed_for_test(
        &self,
        control: &std::sync::Arc<crate::managed::Control>,
    ) {
        assert!(
            self.configuration
                .execution
                .managed
                .set(std::sync::Arc::downgrade(control))
                .is_ok()
        );
    }

    pub(in crate::transport::websocket::session) fn set_before_send_for_test(
        &mut self,
        action: Box<dyn FnOnce() + Send>,
    ) {
        assert!(self.before_send.replace(action).is_none());
    }

    pub(super) fn before_send_for_test(&mut self) {
        if let Some(action) = self.before_send.take() {
            action();
        }
    }

    pub(in crate::transport::websocket::session) fn fail_clock_for_test(&mut self) {
        self.clock.fail_for_test();
    }

    pub(in crate::transport::websocket::session) fn operation_state_for_test(
        &self,
        id: &str,
    ) -> Option<(BridgeGeneration, bool, bool)> {
        self.slots
            .iter()
            .flatten()
            .find(|slot| slot.wire_id() == Some(id))
            .map(|slot| {
                (
                    slot.generation(),
                    slot.live_generation().is_some(),
                    slot.has_event(),
                )
            })
    }

    pub(in crate::transport::websocket::session) fn queued_priority_for_test(&self) -> bool {
        self.parts
            .as_ref()
            .is_some_and(|parts| parts.controls.has_queued(WebSocketControlClass::Priority))
    }

    pub(in crate::transport::websocket::session) fn set_time_for_test(&mut self, now: u64) {
        self.clock.set_for_test(now);
    }

    pub(in crate::transport::websocket::session) fn encoded_bytes_for_test(&self) -> usize {
        self.configuration.owner.used()
    }
}
