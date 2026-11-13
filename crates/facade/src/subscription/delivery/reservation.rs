//! Single-owner pre-poll encoded-byte reservations.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{ByteReservationError, EncodeReservation, EventByteCharge};

/// One pre-poll reservation that clears the controller slot on every drop path.
pub(crate) struct ControllerReservation {
    pub(super) reservation: Option<EncodeReservation>,
    pub(super) held: Arc<AtomicBool>,
}

impl ControllerReservation {
    pub(super) fn shrink_encoded(
        &mut self,
        actual_bytes: usize,
    ) -> Result<(), ByteReservationError> {
        self.reservation
            .as_mut()
            .ok_or(ByteReservationError::Accounting)?
            .shrink_encoded(actual_bytes)
    }

    pub(super) fn into_encoding_reservation(
        mut self,
    ) -> Result<EncodeReservation, ByteReservationError> {
        self.reservation
            .take()
            .ok_or(ByteReservationError::Accounting)
    }

    pub(super) fn commit(
        mut self,
        actual_bytes: usize,
    ) -> Result<EventByteCharge, ByteReservationError> {
        let reservation = self
            .reservation
            .take()
            .ok_or(ByteReservationError::Accounting)?;
        self.held.store(false, Ordering::Release);
        reservation.commit(actual_bytes)
    }
}

impl Drop for ControllerReservation {
    fn drop(&mut self) {
        self.held.store(false, Ordering::Release);
    }
}
