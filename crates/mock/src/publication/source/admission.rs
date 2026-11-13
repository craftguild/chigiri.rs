//! Share source capacity through physical queue and executing-input release.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::publication::PublicationError;

pub(in crate::publication) struct Admission {
    used: Arc<AtomicUsize>,
    limit: usize,
}

pub(super) struct Permit(Arc<AtomicUsize>);

impl Admission {
    pub(in crate::publication) fn new(limit: usize) -> Self {
        Self {
            used: Arc::default(),
            limit,
        }
    }

    pub(super) fn acquire(&self) -> Result<Permit, PublicationError> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(1).filter(|next| *next <= self.limit)
            })
            .map_err(|_| PublicationError::BindingCapacity)?;
        Ok(Permit(self.used.clone()))
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
