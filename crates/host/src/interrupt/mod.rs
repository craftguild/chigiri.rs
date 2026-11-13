//! Platform-neutral process interruption backed by retained registrations.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(windows)]
mod windows;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::HostError;

/// A retained platform interrupt registration.
pub struct Registration {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    _backend: unix::Registration,
    #[cfg(windows)]
    _backend: windows::Registration,
}

/// An owned flag set by TERM/INT or the corresponding console event.
pub struct Interrupt {
    requested: Arc<AtomicBool>,
    _registration: Registration,
}

impl Interrupt {
    /// Installs an interrupt source for an operation that has begun host work.
    pub fn install() -> Result<Self, HostError> {
        let requested = Arc::new(AtomicBool::new(false));
        let registration = register(requested.clone())?;
        Ok(Self {
            requested,
            _registration: registration,
        })
    }

    /// Reports whether the host delivered an interrupt.
    pub fn requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }
}

/// Connects a caller-owned flag to the platform interrupt source.
pub fn register(requested: Arc<AtomicBool>) -> Result<Registration, HostError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::register(requested).map(|_backend| Registration { _backend })
    }
    #[cfg(windows)]
    {
        windows::register(requested).map(|_backend| Registration { _backend })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = requested;
        Err(HostError::Unsupported)
    }
}
