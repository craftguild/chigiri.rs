//! Finite terminal responses release observation once and retain wire ownership.

use chigiri_http::{ResponseMediaType, ResponseParts};
use chigiri_runtime::LifecycleResult;

use super::{PreparedHttpSubscription, SubscriptionInitialResponse};
use crate::subscription::control::initial_response_charge;
use crate::subscription::worker::{
    InitialResponseBufferLease, InitialResponseDeadline, SubscriptionWorkerRuntimeError,
};
use crate::transport::response::timeout_response;
use crate::transport::streaming::InitialRequestObservation;

pub(super) fn cancelled<C: Send + Sync + 'static>(
    observation: InitialRequestObservation<C>,
    buffer: Option<InitialResponseBufferLease>,
) -> PreparedHttpSubscription {
    drop(buffer);
    observation.finish(LifecycleResult::Cancelled, None);
    PreparedHttpSubscription::Response(SubscriptionInitialResponse::empty(503))
}

pub(super) fn observed_failure<C: Send + Sync + 'static>(
    observation: InitialRequestObservation<C>,
    buffer: Option<InitialResponseBufferLease>,
) -> PreparedHttpSubscription {
    drop(buffer);
    observation.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
    PreparedHttpSubscription::Response(SubscriptionInitialResponse::empty(500))
}

pub(super) fn finite<C: Send + Sync + 'static>(
    media: ResponseMediaType,
    response: ResponseParts,
    observation: InitialRequestObservation<C>,
    buffer: Option<InitialResponseBufferLease>,
    capacity_rejection: bool,
) -> PreparedHttpSubscription {
    let Some(mut buffer) = buffer else {
        return closed(observation);
    };
    match buffer.try_finish(&response) {
        Ok(charge) => {
            observe(observation, response.status, capacity_rejection);
            PreparedHttpSubscription::Response(SubscriptionInitialResponse::charged(
                response, charge,
            ))
        }
        Err(error) => {
            let response = observation.worker_failure(media, &error);
            recover(response, observation, buffer)
        }
    }
}

/// Pre-worker admission failure has no subscription buffer or application work.
pub(super) fn admission<C: Send + Sync + 'static>(
    response: ResponseParts,
    observation: InitialRequestObservation<C>,
) -> PreparedHttpSubscription {
    observe(observation, response.status, true);
    PreparedHttpSubscription::Response(SubscriptionInitialResponse::admission(response))
}

pub(super) fn closed<C: Send + Sync + 'static>(
    observation: InitialRequestObservation<C>,
) -> PreparedHttpSubscription {
    observation.finish(LifecycleResult::Failed, Some("INTERNAL_SERVER_ERROR"));
    PreparedHttpSubscription::Failed(SubscriptionWorkerRuntimeError::Invariant)
}

fn recover<C: Send + Sync + 'static>(
    response: ResponseParts,
    observation: InitialRequestObservation<C>,
    mut buffer: InitialResponseBufferLease,
) -> PreparedHttpSubscription {
    let wire = match buffer.try_finish(&response) {
        Ok(charge) => SubscriptionInitialResponse::charged(response, charge),
        Err(_) if initial_response_charge(&response).is_ok() && buffer.retains_maximum() => {
            // Keep the original maximum token when exact-shrink itself is unavailable.
            SubscriptionInitialResponse::reserved(response, buffer)
        }
        Err(_) => return closed(observation),
    };
    observation.finish_status(wire.status());
    PreparedHttpSubscription::Response(wire)
}

pub(super) fn failed<C: Send + Sync + 'static>(
    media: ResponseMediaType,
    observation: InitialRequestObservation<C>,
    buffer: Option<InitialResponseBufferLease>,
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> PreparedHttpSubscription {
    let response = observation.worker_failure(media, error);
    finite(media, response, observation, buffer, false)
}

pub(super) fn timed_out<C: Send + Sync + 'static>(
    media: ResponseMediaType,
    observation: InitialRequestObservation<C>,
    buffer: Option<InitialResponseBufferLease>,
) -> PreparedHttpSubscription {
    finite(media, timeout_response(media), observation, buffer, false)
}

pub(super) fn observe<C: Send + Sync + 'static>(
    observation: InitialRequestObservation<C>,
    status: u16,
    capacity_rejection: bool,
) {
    if capacity_rejection {
        observation.finish(
            LifecycleResult::Rejected,
            Some("SUBSCRIPTION_LIMIT_EXCEEDED"),
        );
    } else {
        observation.finish_status(status);
    }
}

pub(super) fn from_deadline<C: Send + Sync + 'static>(
    media: ResponseMediaType,
    observation: InitialRequestObservation<C>,
    mut deadline: InitialResponseDeadline,
    timed_out: bool,
) -> PreparedHttpSubscription {
    if !timed_out && deadline.task_failed() {
        let buffer = deadline.take_buffer();
        drop(deadline);
        return observed_failure(observation, buffer);
    }
    let response = if timed_out {
        timeout_response(media)
    } else {
        observation.worker_failure(media, &SubscriptionWorkerRuntimeError::Invariant)
    };
    match deadline.finish_bytes(&response) {
        Ok(charge) => {
            // Any live task stays generation-owned when this lease is dropped.
            drop(deadline);
            observation.finish_status(response.status);
            PreparedHttpSubscription::Response(SubscriptionInitialResponse::charged(
                response, charge,
            ))
        }
        Err(_) => {
            let buffer = deadline.take_buffer();
            drop(deadline);
            // An intact maximum buffer owns a bounded masked fallback without re-admission.
            let response =
                observation.worker_failure(media, &SubscriptionWorkerRuntimeError::Invariant);
            match buffer {
                Some(buffer) => recover(response, observation, buffer),
                None => closed(observation),
            }
        }
    }
}
