//! Closed failures for bounded control staging.

use chigiri_http::{JsonConversionError, SubscriptionControlEncodingError};

use super::super::delivery::ByteReservationError;

/// A credential-free outcome from control-frame staging.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionControlSignal {
    /// This staging owner was cancelled.
    Cancelled,
    /// The fixed per-connection control queue is full.
    QueueCapacity,
    /// The endpoint-wide encoded-byte owner has no capacity.
    SharedByteCapacity,
    /// One control cannot fit the intrinsic control-frame ceiling.
    ControlLimit,
    /// A control value could not be encoded safely.
    Encoding,
    /// The selected control is invalid for this transport or input.
    InvalidTransportArguments,
    /// Checked ownership or byte accounting failed.
    Accounting,
}

impl SubscriptionControlSignal {
    pub(super) const fn from_reservation(error: ByteReservationError) -> Self {
        match error {
            ByteReservationError::Capacity => Self::SharedByteCapacity,
            ByteReservationError::Accounting | ByteReservationError::InvalidShrink => {
                Self::Accounting
            }
        }
    }

    pub(super) const fn from_encoding(error: SubscriptionControlEncodingError) -> Self {
        match error {
            SubscriptionControlEncodingError::Json(JsonConversionError::ResponseSizeLimit) => {
                Self::ControlLimit
            }
            SubscriptionControlEncodingError::Json(_)
            | SubscriptionControlEncodingError::Event(_) => Self::Encoding,
            SubscriptionControlEncodingError::InvalidTransportArguments
            | SubscriptionControlEncodingError::InvalidWebSocketOperationId
            | SubscriptionControlEncodingError::MissingErrors => Self::InvalidTransportArguments,
        }
    }
}

impl std::fmt::Display for SubscriptionControlSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "subscription control staging was cancelled",
            Self::QueueCapacity => "subscription control queue capacity was exhausted",
            Self::SharedByteCapacity => "subscription endpoint byte capacity was exhausted",
            Self::ControlLimit => "subscription control exceeded its encoded byte limit",
            Self::Encoding => "subscription control encoding failed",
            Self::InvalidTransportArguments => "subscription control arguments are invalid",
            Self::Accounting => "subscription control accounting failed",
        })
    }
}

impl std::error::Error for SubscriptionControlSignal {}
