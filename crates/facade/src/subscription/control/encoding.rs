//! Reserve-before-encode ownership shared by control staging profiles.

use chigiri_http::{EncodedSubscriptionControl, SubscriptionControlEncodingError};

use super::super::delivery::{EncodeReservation, EndpointByteOwner, EventByteCharge};
use super::SubscriptionControlSignal;

pub(super) struct StagedControl {
    encoded: EncodedSubscriptionControl,
    charge: EventByteCharge,
}

impl StagedControl {
    pub(super) fn len(&self) -> usize {
        self.encoded.len()
    }

    pub(super) fn into_parts(self) -> (EncodedSubscriptionControl, EventByteCharge) {
        (self.encoded, self.charge)
    }
}

pub(super) fn encode_reserved<F>(
    bytes: &EndpointByteOwner,
    reservation_bytes: usize,
    expected_bytes: Option<usize>,
    encode: F,
) -> Result<StagedControl, SubscriptionControlSignal>
where
    F: FnOnce(usize) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError>,
{
    let reservation = bytes
        .try_reserve(reservation_bytes)
        .map_err(SubscriptionControlSignal::from_reservation)?;
    encode_pre_reserved(reservation, reservation_bytes, expected_bytes, encode)
}

pub(super) fn encode_pre_reserved<F>(
    reservation: EncodeReservation,
    maximum_bytes: usize,
    expected_bytes: Option<usize>,
    encode: F,
) -> Result<StagedControl, SubscriptionControlSignal>
where
    F: FnOnce(usize) -> Result<EncodedSubscriptionControl, SubscriptionControlEncodingError>,
{
    if maximum_bytes > reservation.bytes() {
        return Err(SubscriptionControlSignal::Accounting);
    }
    let encoded = encode(maximum_bytes).map_err(SubscriptionControlSignal::from_encoding)?;
    let actual_bytes = encoded.len();
    if actual_bytes > maximum_bytes || expected_bytes.is_some_and(|value| value != actual_bytes) {
        return Err(SubscriptionControlSignal::Accounting);
    }
    let charge = reservation
        .commit(actual_bytes)
        .map_err(SubscriptionControlSignal::from_reservation)?;
    if charge.bytes() != actual_bytes {
        return Err(SubscriptionControlSignal::Accounting);
    }
    Ok(StagedControl { encoded, charge })
}
