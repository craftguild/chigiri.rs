//! Response owners are declared before timeline guards, including on unwind.

use std::sync::Arc;

use chigiri_runtime::GraphQlResponse;

use super::BodyPort;

pub(super) struct ResponsePayload {
    pub(super) value: Option<GraphQlResponse>,
    #[cfg(test)]
    port: std::sync::Weak<BodyPort>,
    #[cfg(test)]
    dropped: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl ResponsePayload {
    pub(super) fn new(
        value: Option<GraphQlResponse>,
        _port: &Arc<BodyPort>,
        _terminal: bool,
    ) -> Self {
        Self {
            value,
            #[cfg(test)]
            port: Arc::downgrade(_port),
            #[cfg(test)]
            dropped: _port.payload_probe_for_test(_terminal),
        }
    }
}

#[cfg(test)]
impl Drop for ResponsePayload {
    fn drop(&mut self) {
        if self.value.is_none() {
            return;
        }
        if let (Some(port), Some(dropped)) = (self.port.upgrade(), &self.dropped) {
            let unlocked = !matches!(
                port.timeline.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            );
            dropped.store(
                if unlocked { 1 } else { 2 },
                std::sync::atomic::Ordering::Release,
            );
        }
    }
}
