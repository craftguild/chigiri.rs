//! Unix TERM and INT registration.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use signal_hook::consts::{SIGINT, SIGTERM};

use crate::HostError;

pub(super) struct Registration(Vec<signal_hook::SigId>);

pub(super) fn register(requested: Arc<AtomicBool>) -> Result<Registration, HostError> {
    let mut registrations = Registration(Vec::new());
    for signal in [SIGTERM, SIGINT] {
        let registration =
            signal_hook::flag::register(signal, requested.clone()).map_err(|_| HostError::Io)?;
        registrations.0.push(registration);
    }
    Ok(registrations)
}

impl Drop for Registration {
    fn drop(&mut self) {
        for registration in self.0.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}
