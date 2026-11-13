//! Source terminal output retains its pre-demand reservation through encoding.

use std::sync::Arc;

use chigiri_http::{
    __encode_subscription_control, __subscription_static_control_len, SubscriptionControl,
    SubscriptionStaticControl, SubscriptionTransport,
};
use chigiri_runtime::GraphQlResponse;

use super::super::delivery::{EncodeReservation, EndpointByteOwner};
use super::encoding::{StagedControl, encode_pre_reserved, encode_reserved};
use super::frame::InFlightGate;
use super::{SubscriptionControlFrame, SubscriptionControlSignal};

const MAX_CONTROL_BYTES: usize = 64 * 1024;

/// One exact source-demand reservation retired from the common HTTP outbox.
///
/// This move-only capability exposes neither its charge nor an encoder bypass.
/// It can produce natural completion or a safe terminal source error once.
pub struct HttpSubscriptionTerminal {
    reservation: EncodeReservation,
    bytes: EndpointByteOwner,
    transport: SubscriptionTransport,
    dropped_total: Option<u64>,
}

impl HttpSubscriptionTerminal {
    pub(crate) const fn new(
        reservation: EncodeReservation,
        bytes: EndpointByteOwner,
        transport: SubscriptionTransport,
        dropped_total: Option<u64>,
    ) -> Self {
        Self {
            reservation,
            bytes,
            transport,
            dropped_total,
        }
    }

    /// Encodes natural completion with exact, non-evictable physical ownership.
    ///
    /// If the event reservation is smaller than this static control, replace
    /// it with the known exact reservation before encoding any control bytes.
    pub fn complete(self) -> Result<HttpTerminalStaging, SubscriptionControlSignal> {
        let length = complete_length(self.transport)?;
        let reservation = if self.reservation.bytes() >= length {
            self.reservation
        } else {
            drop(self.reservation);
            self.bytes
                .try_reserve(length)
                .map_err(SubscriptionControlSignal::from_reservation)?
        };
        let first = encode_pre_reserved(reservation, length, Some(length), |maximum| {
            complete(self.transport, maximum)
        })?;
        Ok(HttpTerminalStaging::new(
            self.bytes,
            self.transport,
            first,
            false,
        ))
    }

    /// Encodes a safe source error, then schedules a separate completion entry.
    ///
    /// Encoding remains within the existing source reservation and the 64-KiB
    /// control ceiling. Failure returns no bytes. SSE retains the current drop
    /// count; multipart projects only its top-level transport error fields.
    pub fn error(
        self,
        response: &GraphQlResponse,
    ) -> Result<HttpTerminalStaging, SubscriptionControlSignal> {
        let maximum = self.reservation.bytes().min(MAX_CONTROL_BYTES);
        let first = encode_pre_reserved(self.reservation, maximum, None, |maximum| {
            __encode_subscription_control(
                self.transport,
                SubscriptionControl::Error {
                    websocket_operation_id: None,
                    response,
                    dropped_total: self.dropped_total,
                },
                maximum,
            )
        })?;
        Ok(HttpTerminalStaging::new(
            self.bytes,
            self.transport,
            first,
            true,
        ))
    }
}

/// At most one physical HTTP terminal control plus one completion marker.
///
/// Neither errors nor completion entries are event-queue entries or evictable.
/// The next entry cannot be encoded until the previous physical owner drops.
pub struct HttpTerminalStaging {
    bytes: EndpointByteOwner,
    transport: SubscriptionTransport,
    first: Option<StagedControl>,
    complete_pending: bool,
    in_flight: Arc<InFlightGate>,
    cancelled: bool,
}

impl HttpTerminalStaging {
    fn new(
        bytes: EndpointByteOwner,
        transport: SubscriptionTransport,
        first: StagedControl,
        complete_pending: bool,
    ) -> Self {
        Self {
            bytes,
            transport,
            first: Some(first),
            complete_pending,
            in_flight: Arc::new(InFlightGate::default()),
            cancelled: false,
        }
    }

    /// Moves the next control into zero-copy physical ownership at a boundary.
    pub fn begin_send(
        &mut self,
    ) -> Result<Option<SubscriptionControlFrame>, SubscriptionControlSignal> {
        if self.cancelled {
            return Err(SubscriptionControlSignal::Cancelled);
        }
        if self.first.is_none() && !self.complete_pending {
            return Ok(None);
        }
        let Some(lease) = self.in_flight.try_acquire() else {
            return Ok(None);
        };
        let staged = if let Some(first) = self.first.take() {
            first
        } else {
            let length = complete_length(self.transport)?;
            // A failure is sticky: no retry or uncharged completion can escape.
            self.cancelled = true;
            let staged = encode_reserved(&self.bytes, length, Some(length), |maximum| {
                complete(self.transport, maximum)
            })?;
            self.complete_pending = false;
            self.cancelled = false;
            staged
        };
        let (encoded, charge) = staged.into_parts();
        Ok(Some(SubscriptionControlFrame::new(
            encoded,
            charge.into_in_flight(),
            lease,
        )))
    }

    /// Reports EOF only after the final physical control owner is destroyed.
    pub fn is_finished(&self) -> bool {
        self.first.is_none() && !self.complete_pending && !self.in_flight.is_busy()
    }

    /// Discards only unoffered controls; returned frames retain their charges.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.complete_pending = false;
        self.first.take();
    }
}

fn complete_length(transport: SubscriptionTransport) -> Result<usize, SubscriptionControlSignal> {
    __subscription_static_control_len(transport, SubscriptionStaticControl::Complete)
        .map_err(SubscriptionControlSignal::from_encoding)
}

fn complete(
    transport: SubscriptionTransport,
    maximum: usize,
) -> Result<chigiri_http::EncodedSubscriptionControl, chigiri_http::SubscriptionControlEncodingError>
{
    __encode_subscription_control(
        transport,
        SubscriptionControl::Complete {
            websocket_operation_id: None,
        },
        maximum,
    )
}
