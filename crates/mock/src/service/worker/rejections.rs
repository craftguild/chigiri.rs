//! Retain the highest-status valid rejection independently of any reply future's lifetime.

use std::sync::{Arc, Mutex};

use super::Client;
use crate::hooks::Rejection;

#[derive(Default)]
pub(in crate::service) struct Rejections(Mutex<Option<Rejection>>);

impl Rejections {
    pub(super) fn record(&self, rejection: &Rejection) {
        if rejection.invalid {
            return;
        }
        let mut current = self.0.lock().expect("rejection ledger lock poisoned");
        if current
            .as_ref()
            .is_none_or(|old| old.status < rejection.status)
        {
            // Equal statuses retain the earlier rejection and its already-decoded public message.
            *current = Some(rejection.clone());
        }
    }

    pub(in crate::service) fn get(&self) -> Option<Rejection> {
        self.0
            .lock()
            .expect("rejection ledger lock poisoned")
            .clone()
    }

    pub(super) fn any(&self) -> bool {
        self.0
            .lock()
            .expect("rejection ledger lock poisoned")
            .is_some()
    }
}

impl Client {
    /// Return the small rejection ledger without creating a `Request`/`Pending` ownership cycle.
    pub(in crate::service) fn rejections(&self) -> Option<Arc<Rejections>> {
        self.request
            .as_ref()
            .map(|request| request.rejections.clone())
    }
}
