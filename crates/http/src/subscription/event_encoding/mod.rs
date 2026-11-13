//! Bounded transport framing for one subscription execution result.

mod framing;
mod metadata;

use std::fmt;
use std::ops::Range;

use chigiri_core::GraphQlResponse;

use crate::response::write_response_with_extensions;
use crate::response_writer::BoundedWriter;
use crate::{JsonConversionError, SubscriptionTransport};

const MAX_EXACT_JSON_INTEGER: u64 = 9_007_199_254_740_991;
const DELIVERY_COUNTER_WIDTH: usize = 16;

/// An encoded event that has not started transmission.
///
/// This value deliberately exposes no provisional bytes. Finalization consumes
/// it so delivery metadata cannot change after the event becomes in-flight.
#[doc(hidden)]
pub struct EncodedSubscriptionEvent {
    bytes: Vec<u8>,
    counter_slot: Range<usize>,
}

/// An immutable, fully charged event ready for partial transport writes.
///
/// The private owner is move-only. Its bytes can only be borrowed immutably,
/// so framing length and framework-owned metadata remain fixed in-flight.
#[doc(hidden)]
pub struct FinalizedSubscriptionEvent {
    bytes: Vec<u8>,
}

impl EncodedSubscriptionEvent {
    /// Returns the exact charged byte length, including transport framing.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether this event occupies no bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Inserts the current delivery count and yields immutable send bytes.
    pub fn finalize(
        mut self,
        dropped_total: Option<u64>,
    ) -> Result<FinalizedSubscriptionEvent, SubscriptionEventEncodingError> {
        let replacement = encoded_counter(dropped_total)?;
        self.bytes[self.counter_slot].copy_from_slice(&replacement);
        Ok(FinalizedSubscriptionEvent { bytes: self.bytes })
    }
}

impl fmt::Debug for EncodedSubscriptionEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncodedSubscriptionEvent")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl FinalizedSubscriptionEvent {
    /// Returns the exact charged byte length, including transport framing.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether this event occupies no bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Borrows the immutable bytes for a transport write.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl AsRef<[u8]> for FinalizedSubscriptionEvent {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for FinalizedSubscriptionEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FinalizedSubscriptionEvent")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// A safe failure from bounded subscription event encoding or finalization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum SubscriptionEventEncodingError {
    /// The shared bounded JSON encoder rejected the result.
    Json(JsonConversionError),
    /// The selected transport and WebSocket operation ID disagree.
    InvalidTransportArguments,
    /// An application-owned extension blocks the reserved object path.
    DeliveryExtensionCollision,
    /// The delivery counter is not exactly representable by supported clients.
    DeliveryCounterOutOfRange,
}

impl fmt::Display for SubscriptionEventEncodingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => error.fmt(formatter),
            Self::InvalidTransportArguments => {
                formatter.write_str("subscription transport arguments are invalid")
            }
            Self::DeliveryExtensionCollision => {
                formatter.write_str("reserved subscription delivery extension has an invalid shape")
            }
            Self::DeliveryCounterOutOfRange => {
                formatter.write_str("subscription delivery counter is outside the supported range")
            }
        }
    }
}

impl std::error::Error for SubscriptionEventEncodingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<JsonConversionError> for SubscriptionEventEncodingError {
    fn from(error: JsonConversionError) -> Self {
        Self::Json(error)
    }
}

pub(crate) fn encode(
    transport: SubscriptionTransport,
    websocket_operation_id: Option<&str>,
    response: &GraphQlResponse,
    maximum_bytes: usize,
) -> Result<EncodedSubscriptionEvent, SubscriptionEventEncodingError> {
    let frame = framing::EventFrame::select(transport, websocket_operation_id)?;
    let mut output = BoundedWriter::new(maximum_bytes);
    let counter_slot = write_to(&mut output, frame, response)?;

    Ok(EncodedSubscriptionEvent {
        bytes: output.finish(),
        counter_slot,
    })
}

/// Reuses one precharged writer when an initial error also appends completion.
pub(super) fn write_http_error(
    output: &mut BoundedWriter,
    response: &GraphQlResponse,
) -> Result<(), SubscriptionEventEncodingError> {
    // A pre-source error has discarded no event; the reserved slot stays null.
    write_to(output, framing::EventFrame::ServerSentEvents, response)?;
    Ok(())
}

fn write_to(
    output: &mut BoundedWriter,
    frame: framing::EventFrame<'_>,
    response: &GraphQlResponse,
) -> Result<Range<usize>, SubscriptionEventEncodingError> {
    frame.write_prefix(output)?;

    let mut counter_slot = None;
    write_response_with_extensions::<SubscriptionEventEncodingError>(
        output,
        response,
        true,
        |output, extensions| {
            counter_slot = Some(metadata::write_extensions(output, extensions)?);
            Ok(())
        },
    )?;
    frame.write_suffix(output)?;
    Ok(counter_slot.expect("metadata writer always reserves the counter slot"))
}

fn encoded_counter(
    dropped_total: Option<u64>,
) -> Result<[u8; DELIVERY_COUNTER_WIDTH], SubscriptionEventEncodingError> {
    let mut encoded = [b' '; DELIVERY_COUNTER_WIDTH];
    let Some(mut value) = dropped_total else {
        encoded[..4].copy_from_slice(b"null");
        return Ok(encoded);
    };
    if value > MAX_EXACT_JSON_INTEGER {
        return Err(SubscriptionEventEncodingError::DeliveryCounterOutOfRange);
    }

    let mut first_digit = encoded.len();
    loop {
        first_digit -= 1;
        encoded[first_digit] = b'0' + u8::try_from(value % 10).expect("one decimal digit");
        value /= 10;
        if value == 0 {
            break;
        }
    }
    let digit_count = encoded.len() - first_digit;
    encoded.copy_within(first_digit.., 0);
    encoded[digit_count..].fill(b' ');
    Ok(encoded)
}
