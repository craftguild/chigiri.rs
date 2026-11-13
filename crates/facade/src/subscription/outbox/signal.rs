//! Safe closed outcomes exposed to official streaming bridges.

use chigiri_http::{JsonConversionError, SubscriptionEventEncodingError};

use super::super::delivery::{DeliveryDecision, TerminationReason};

/// A bounded-outbox outcome that contains no application data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionOutboxSignal {
    /// Source demand must wait for capacity or a policy deadline.
    Wait,
    /// The subscription owner cancelled this generation.
    Cancelled,
    /// A bridge supplied a time earlier than its previous observation.
    NonMonotonicTime,
    /// Transport framing arguments do not identify one supported profile.
    InvalidTransportArguments,
    /// Application extensions conflict with the reserved delivery path.
    DeliveryExtensionCollision,
    /// A GraphQL result could not be encoded safely.
    Encoding,
    /// Internal delivery ownership violated a state invariant.
    Invariant,
    /// Checked byte or queue accounting failed.
    Accounting,
    /// An exact JavaScript delivery counter was exhausted.
    CounterExhausted,
    /// One complete framed event exceeded its configured cap.
    OversizedEvent,
    /// The oldest wholly unsent event exceeded its queue age.
    QueueAgeExpired,
    /// A partial send made no accepted-byte progress before its deadline.
    SendStalled,
    /// Endpoint-wide byte pressure exceeded its finite wait.
    SharedBudgetPressureExpired,
    /// A non-evicting capacity decision rejected the transition.
    CapacityRejected,
}

impl SubscriptionOutboxSignal {
    pub(super) fn from_decision(decision: DeliveryDecision) -> Self {
        match decision {
            DeliveryDecision::Pause => Self::Wait,
            DeliveryDecision::RejectNew => Self::CapacityRejected,
            DeliveryDecision::Terminate(reason) => Self::from_termination(reason),
            DeliveryDecision::Admit | DeliveryDecision::EvictWhole(_) => Self::Invariant,
        }
    }

    pub(super) fn from_encoding(error: SubscriptionEventEncodingError) -> Self {
        match error {
            SubscriptionEventEncodingError::Json(JsonConversionError::ResponseSizeLimit) => {
                Self::OversizedEvent
            }
            SubscriptionEventEncodingError::Json(_) => Self::Encoding,
            SubscriptionEventEncodingError::InvalidTransportArguments => {
                Self::InvalidTransportArguments
            }
            SubscriptionEventEncodingError::DeliveryExtensionCollision => {
                Self::DeliveryExtensionCollision
            }
            SubscriptionEventEncodingError::DeliveryCounterOutOfRange => Self::CounterExhausted,
        }
    }

    pub(super) const fn decision_for_encoding(self) -> DeliveryDecision {
        let reason = match self {
            Self::CounterExhausted => TerminationReason::CounterExhausted,
            Self::OversizedEvent => TerminationReason::OversizedEvent,
            Self::Accounting => TerminationReason::Accounting,
            _ => TerminationReason::Invariant,
        };
        DeliveryDecision::Terminate(reason)
    }

    const fn from_termination(reason: TerminationReason) -> Self {
        match reason {
            TerminationReason::Invariant => Self::Invariant,
            TerminationReason::Accounting => Self::Accounting,
            TerminationReason::CounterExhausted => Self::CounterExhausted,
            TerminationReason::OversizedEvent => Self::OversizedEvent,
            TerminationReason::QueueAgeExpired => Self::QueueAgeExpired,
            TerminationReason::SendStalled => Self::SendStalled,
            TerminationReason::SharedBudgetPressureExpired => Self::SharedBudgetPressureExpired,
        }
    }
}

impl std::fmt::Display for SubscriptionOutboxSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Wait => "subscription delivery is waiting for capacity",
            Self::Cancelled => "subscription delivery was cancelled",
            Self::NonMonotonicTime => "subscription delivery time moved backwards",
            Self::InvalidTransportArguments => "subscription transport arguments are invalid",
            Self::DeliveryExtensionCollision => {
                "reserved subscription delivery extensions have an invalid shape"
            }
            Self::Encoding => "subscription event encoding failed",
            Self::Invariant => "subscription delivery invariant failed",
            Self::Accounting => "subscription delivery accounting failed",
            Self::CounterExhausted => "subscription delivery counter was exhausted",
            Self::OversizedEvent => "subscription event exceeded its encoded byte limit",
            Self::QueueAgeExpired => "subscription queue age expired",
            Self::SendStalled => "subscription send progress expired",
            Self::SharedBudgetPressureExpired => {
                "subscription endpoint byte pressure deadline expired"
            }
            Self::CapacityRejected => "subscription delivery capacity rejected the transition",
        })
    }
}

impl std::error::Error for SubscriptionOutboxSignal {}
