//! Windows console interrupt registration through one process-global handler.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::HostError;

static FLAGS: OnceLock<Mutex<Vec<Weak<AtomicBool>>>> = OnceLock::new();
static HANDLER: OnceLock<Result<(), ()>> = OnceLock::new();

pub(super) struct Registration(Weak<AtomicBool>);

pub(super) fn register(requested: Arc<AtomicBool>) -> Result<Registration, HostError> {
    HANDLER
        .get_or_init(|| {
            ctrlc::set_handler(|| {
                let mut flags = flags().lock().unwrap_or_else(|error| error.into_inner());
                flags.retain(|flag| {
                    if let Some(flag) = flag.upgrade() {
                        flag.store(true, Ordering::SeqCst);
                        true
                    } else {
                        false
                    }
                });
            })
            .map_err(|_| ())
        })
        .map_err(|()| HostError::Io)?;
    let registration = Registration(Arc::downgrade(&requested));
    flags()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(registration.0.clone());
    Ok(registration)
}

impl Drop for Registration {
    fn drop(&mut self) {
        flags()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|flag| !Weak::ptr_eq(flag, &self.0));
    }
}

fn flags() -> &'static Mutex<Vec<Weak<AtomicBool>>> {
    FLAGS.get_or_init(|| Mutex::new(Vec::new()))
}
