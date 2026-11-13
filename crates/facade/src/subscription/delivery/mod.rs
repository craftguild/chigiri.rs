//! Shared, transport-neutral subscription delivery ownership.

mod bytes;
mod cleanup;
mod controller;
mod controller_policy;
mod counter;
mod evaluation;
mod permits;
mod queue;
mod reservation;
mod time;

pub(crate) use bytes::{
    ByteReservationError, EncodeReservation, EndpointByteOwner, EventByteCharge, InFlightByteCharge,
};
pub(crate) use cleanup::{
    CancellationFlag, CleanupPhysicalCharge, CleanupTicket, EmissionAuthority, QuarantineError,
    QuarantineKey, QuarantineRegistry,
};
pub(crate) use controller::DeliveryController;
pub(crate) use counter::CheckedCounter;
pub(crate) use evaluation::{
    DeliveryDecision, DeliverySnapshot, EvaluationPoint, TerminationReason, evaluate,
};
#[cfg(test)]
pub(crate) use permits::QuotaSnapshot;
#[cfg(feature = "subscription-worker")]
pub(crate) use permits::SharedConnectionPhysicalLease;
pub(crate) use permits::{
    ActiveConnectionOperation, ConnectionAdmissionResources, ConnectionResources,
    DeliveryResources, OperationKind, OwnerGeneration, PendingConnectionOperation,
    PendingEndpointOperation, PermitError, RevokedConnectionCharge, RevokedConnectionOwner,
    RevokedPendingConnectionCharge, RevokedPendingEndpointCharge, SharedActiveConnectionPermit,
    SharedActiveSubscriptionPermit,
};
pub(crate) use queue::{EventQueue, InFlightProgress, InFlightSend, InFlightSlot, QueuedEvent};
pub(crate) use reservation::ControllerReservation;
pub(crate) use time::{MonotonicInstant, SharedPressureWait};

#[cfg(test)]
mod tests;
