//! Checked endpoint-wide encoded-byte ownership and move-only charge transfer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// One endpoint-wide encoded-byte owner shared by endpoint clones.
#[derive(Clone, Debug)]
pub(crate) struct EndpointByteOwner {
    inner: Arc<ByteOwnerInner>,
}

#[derive(Debug)]
struct ByteOwnerInner {
    limit: usize,
    used: AtomicUsize,
    corrupted: AtomicBool,
}

impl EndpointByteOwner {
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new(ByteOwnerInner {
                limit,
                used: AtomicUsize::new(0),
                corrupted: AtomicBool::new(false),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn limit(&self) -> usize {
        self.inner.limit
    }

    pub(crate) fn used(&self) -> usize {
        self.inner.used.load(Ordering::Acquire)
    }

    pub(crate) fn is_corrupted(&self) -> bool {
        self.inner.corrupted.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn corrupt_for_test(&self) {
        self.inner.corrupted.store(true, Ordering::Release);
    }

    pub(crate) fn try_reserve(
        &self,
        bytes: usize,
    ) -> Result<EncodeReservation, ByteReservationError> {
        if self.is_corrupted() {
            return Err(ByteReservationError::Accounting);
        }
        let mut current = self.used();
        loop {
            let next = current
                .checked_add(bytes)
                .ok_or(ByteReservationError::Accounting)?;
            if next > self.inner.limit {
                return Err(ByteReservationError::Capacity);
            }
            match self.inner.used.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(EncodeReservation {
                        charge: Some(OwnedByteCharge {
                            owner: Arc::clone(&self.inner),
                            bytes,
                        }),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

/// Failure to reserve or transfer checked encoded-byte ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ByteReservationError {
    Capacity,
    Accounting,
    InvalidShrink,
}

/// Maximum-size ownership acquired before a source is polled.
#[derive(Debug)]
pub(crate) struct EncodeReservation {
    charge: Option<OwnedByteCharge>,
}

impl EncodeReservation {
    pub(crate) fn shrink_encoded(
        &mut self,
        actual_bytes: usize,
    ) -> Result<(), ByteReservationError> {
        self.charge
            .as_mut()
            .ok_or(ByteReservationError::Accounting)?
            .shrink_to(actual_bytes)
    }

    pub(crate) fn bytes(&self) -> usize {
        self.charge.as_ref().map_or(0, |charge| charge.bytes)
    }

    pub(crate) fn commit(
        mut self,
        actual_bytes: usize,
    ) -> Result<EventByteCharge, ByteReservationError> {
        self.try_commit(actual_bytes)
    }

    /// Keeps the original reservation available when validation or shrink fails.
    pub(crate) fn try_commit(
        &mut self,
        actual_bytes: usize,
    ) -> Result<EventByteCharge, ByteReservationError> {
        let charge = self
            .charge
            .as_mut()
            .ok_or(ByteReservationError::Accounting)?;
        if charge.bytes != actual_bytes {
            charge.shrink_to(actual_bytes)?;
        } else if charge.owner.corrupted.load(Ordering::Acquire) {
            return Err(ByteReservationError::Accounting);
        }
        Ok(EventByteCharge {
            charge: self.charge.take(),
        })
    }

    #[cfg(feature = "subscription-worker")]
    pub(crate) fn retains(&self, bytes: usize) -> bool {
        self.charge.as_ref().is_some_and(|charge| {
            charge.bytes == bytes && !charge.owner.corrupted.load(Ordering::Acquire)
        })
    }
}

/// Exact charge for one wholly encoded, still-evictable event.
#[derive(Debug)]
pub(crate) struct EventByteCharge {
    charge: Option<OwnedByteCharge>,
}

impl EventByteCharge {
    pub(crate) fn bytes(&self) -> usize {
        self.charge.as_ref().map_or(0, |charge| charge.bytes)
    }

    pub(crate) fn into_in_flight(mut self) -> InFlightByteCharge {
        InFlightByteCharge {
            charge: self.charge.take(),
        }
    }
}

/// Exact non-evictable charge retained through the final accepted byte.
#[derive(Debug)]
pub(crate) struct InFlightByteCharge {
    charge: Option<OwnedByteCharge>,
}

impl InFlightByteCharge {
    pub(crate) fn bytes(&self) -> usize {
        self.charge.as_ref().map_or(0, |charge| charge.bytes)
    }
}

#[derive(Debug)]
struct OwnedByteCharge {
    owner: Arc<ByteOwnerInner>,
    bytes: usize,
}

impl OwnedByteCharge {
    fn shrink_to(&mut self, actual_bytes: usize) -> Result<(), ByteReservationError> {
        if self.owner.corrupted.load(Ordering::Acquire) {
            return Err(ByteReservationError::Accounting);
        }
        if actual_bytes > self.bytes {
            return Err(ByteReservationError::InvalidShrink);
        }
        let released = self.bytes - actual_bytes;
        release(&self.owner, released)?;
        self.bytes = actual_bytes;
        if self.owner.corrupted.load(Ordering::Acquire) {
            Err(ByteReservationError::Accounting)
        } else {
            Ok(())
        }
    }
}

impl Drop for OwnedByteCharge {
    fn drop(&mut self) {
        let _ = release(&self.owner, self.bytes);
    }
}

fn release(owner: &ByteOwnerInner, bytes: usize) -> Result<(), ByteReservationError> {
    let mut current = owner.used.load(Ordering::Acquire);
    loop {
        let Some(next) = current.checked_sub(bytes) else {
            owner.corrupted.store(true, Ordering::Release);
            return Err(ByteReservationError::Accounting);
        };
        match owner
            .used
            .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => return Ok(()),
            Err(observed) => current = observed,
        }
    }
}
