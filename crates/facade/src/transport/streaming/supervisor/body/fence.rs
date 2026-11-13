//! The managed endpoint's emission fence survives supervisor scheduling delay.

use std::sync::{Arc, Weak};

use super::BodyPort;
use crate::managed::Control;

impl BodyPort {
    pub(in crate::transport::streaming::supervisor) fn bind_control(&self, control: &Arc<Control>) {
        let _ = self.control.set(Arc::downgrade(control));
    }

    pub(in crate::transport::streaming::supervisor) fn control(&self) -> Option<Arc<Control>> {
        self.control.get().and_then(Weak::upgrade)
    }
}
