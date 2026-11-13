//! Move-only setup and event results returned to the protocol supervisor.

use chigiri_http::{
    ResponseCompletionOutcome, ResponseHeaders, ResponseParts, SubscriptionTransport,
};
use chigiri_runtime::{GraphQlResponse, SubscriptionOverflow};

use crate::subscription::driver::SubscriptionOutput;
use crate::subscription::outbox::SubscriptionEventReservation;
use crate::transport::streaming::InitialRequestObservation;

use super::InitialResponseDeadline;
use crate::subscription::worker::InitialResponseBufferLease;

pub(super) enum WorkerSetupMessage {
    Observation(chigiri_runtime::__private::SubscriptionObservation),
    Ready {
        headers: ResponseHeaders,
        overflow: SubscriptionOverflow,
        receipt: Option<chigiri_runtime::__private::SubscriptionObservationReceipt>,
        telemetry: Option<chigiri_runtime::RequestTelemetry>,
    },
    Response {
        response: GraphQlResponse,
        headers: ResponseHeaders,
    },
    CompletionResponse {
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    },
    Rejected(ResponseParts),
}

impl crate::subscription::worker::observation::HostObservationHandoff
    for super::PortSender<WorkerSetupMessage>
{
    fn transfer(
        &self,
        observation: chigiri_runtime::__private::SubscriptionObservation,
    ) -> Result<(), chigiri_runtime::__private::SubscriptionObservation> {
        self.try_send(WorkerSetupMessage::Observation(observation))
            .map_err(|message| {
                let WorkerSetupMessage::Observation(observation) = message else {
                    unreachable!()
                };
                observation
            })
    }
}

/// Setup result with the sole continuing source owner.
pub(crate) struct SubscriptionWorkerSetup<C: Send + Sync + 'static> {
    pub(crate) deadline: InitialResponseDeadline,
    pub(crate) headers: ResponseHeaders,
    pub(crate) overflow: SubscriptionOverflow,
    pub(crate) observation: InitialRequestObservation<C>,
    pub(crate) transport: SubscriptionTransport,
}

/// One demanded source result paired with its original byte reservation.
pub(crate) struct SubscriptionWorkerEvent {
    reservation: SubscriptionEventReservation,
    output: SubscriptionOutput,
}

impl SubscriptionWorkerEvent {
    pub(super) const fn new(
        reservation: SubscriptionEventReservation,
        output: SubscriptionOutput,
    ) -> Self {
        Self {
            reservation,
            output,
        }
    }

    pub(crate) fn into_parts(self) -> (SubscriptionEventReservation, SubscriptionOutput) {
        (self.reservation, self.output)
    }
}

pub(crate) enum ActiveSetupOutcome<C: Send + Sync + 'static> {
    ObservedFailure {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
    Cancelled {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
    Ready(SubscriptionWorkerSetup<C>),
    Response {
        deadline: InitialResponseDeadline,
        response: GraphQlResponse,
        headers: ResponseHeaders,
        observation: InitialRequestObservation<C>,
        transport: SubscriptionTransport,
    },
    CompletionResponse {
        deadline: InitialResponseDeadline,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
        observation: InitialRequestObservation<C>,
    },
    Rejected {
        deadline: InitialResponseDeadline,
        response: ResponseParts,
        observation: InitialRequestObservation<C>,
    },
    TimedOut {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
    Failed {
        observation: InitialRequestObservation<C>,
        buffer: Option<InitialResponseBufferLease>,
    },
}

pub(crate) enum ActiveWorkerSignal {
    Cancelled,
    Event(SubscriptionWorkerEvent),
    Finished,
    TimedOut,
    Failed,
}

pub(crate) struct ActiveSetupFailure<C: Send + Sync + 'static> {
    pub(crate) kind: ActiveSetupFailureKind,
    pub(crate) observation: InitialRequestObservation<C>,
    pub(crate) deadline: Option<InitialResponseDeadline>,
    pub(crate) buffer: Option<InitialResponseBufferLease>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActiveSetupFailureKind {
    Capacity,
    Failed,
}
