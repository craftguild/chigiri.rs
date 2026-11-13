//! Keep executing input bytes charged until the actual event owner is dropped.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::cancellation::Cancellation;
use crate::events::retention::{Reservation, Retention};
use crate::publication::PublicationError;

pub(in crate::publication) struct Usage {
    bytes: AtomicUsize,
    pub(in crate::publication) closed: AtomicBool,
    pub(in crate::publication) cancel: Cancellation,
    permit: Mutex<Option<super::admission::Permit>>,
    _reservation: Reservation,
}

pub(in crate::publication) struct DeliveryLease {
    usage: Arc<Usage>,
    bytes: usize,
    _reservation: Reservation,
}

impl Usage {
    pub(super) fn new(retention: &Retention) -> Result<Arc<Self>, PublicationError> {
        let reservation = retention
            .reserve(
                size_of::<Self>()
                    + 2 * size_of::<usize>()
                    + Cancellation::retained_allocation_bytes(),
                usize::MAX,
            )
            .map_err(|_| PublicationError::CaptureLimit)?;
        Ok(Arc::new(Self {
            bytes: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            cancel: Cancellation::default(),
            permit: Mutex::new(None),
            _reservation: reservation,
        }))
    }

    /// Atomic inspection only: the caller may hold the store linearization lock.
    pub(in crate::publication) fn retains_input(&self) -> bool {
        !self.closed.load(Ordering::Acquire) && self.bytes() > 0
    }

    pub(super) fn bytes(&self) -> usize {
        self.bytes.load(Ordering::Acquire)
    }

    pub(super) fn admit(
        &self,
        admission: &super::admission::Admission,
    ) -> Result<(), PublicationError> {
        let mut permit = self
            .permit
            .lock()
            .map_err(|_| PublicationError::Invariant)?;
        if permit.is_some() {
            return Err(PublicationError::Invariant);
        }
        *permit = Some(admission.acquire()?);
        Ok(())
    }
}

impl DeliveryLease {
    pub(super) fn reserve(
        usage: &Arc<Usage>,
        retention: &Retention,
        bytes: usize,
        limit: usize,
    ) -> Result<Self, PublicationError> {
        let reservation = retention
            .reserve(
                size_of::<Self>() + size_of::<super::super::CapturedEvent>(),
                usize::MAX,
            )
            .map_err(|_| PublicationError::CaptureLimit)?;
        usage
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= limit)
            })
            .map_err(|_| PublicationError::CaptureLimit)?;
        Ok(Self {
            usage: usage.clone(),
            bytes,
            _reservation: reservation,
        })
    }

    pub(in crate::publication) fn cancellation(&self) -> &Cancellation {
        &self.usage.cancel
    }
}

impl Drop for DeliveryLease {
    fn drop(&mut self) {
        self.usage.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
