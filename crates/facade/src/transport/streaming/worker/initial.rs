//! Bounded framing, deadline-first commit, and one task-local completion exchange.

use std::future::poll_fn;
use std::task::Poll;

use chigiri_http::{
    __encode_subscription_http_initial, JsonConversionError, ResponseCompletionOutcome,
    ResponseHeaders, ResponseParts, SubscriptionControlEncodingError,
    SubscriptionEventEncodingError, SubscriptionTransport,
};
use chigiri_runtime::GraphQlResponse;

use super::event::HttpSubscriptionEvents;
use super::{
    HttpWorkerConfiguration, PreparedHttpSubscription, ReadyHttpSubscription,
    SubscriptionInitialResponse, failure,
};
use crate::subscription::control::MAX_INITIAL_RESPONSE_BYTES;
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::{
    ActiveSetupOutcome, InitialResponseDeadline, InitialResponseGate,
    SubscriptionWorkerRuntimeError,
};
use crate::transport::streaming::InitialRequestObservation;

pub(super) struct InitialPlan<C: Send + Sync + 'static> {
    pub(super) deadline: InitialResponseDeadline,
    pub(super) response: ResponseParts,
    pub(super) observation: InitialRequestObservation<C>,
    pub(super) completion: Option<ResponseCompletionOutcome>,
    pub(super) source: bool,
    pub(super) outbox: Option<SubscriptionOutbox>,
    pub(super) capacity_rejection: bool,
    pub(super) profile: Option<SubscriptionTransport>,
}

pub(super) async fn active<C: Send + Sync + 'static>(
    config: &HttpWorkerConfiguration,
    setup: ActiveSetupOutcome<C>,
) -> PreparedHttpSubscription {
    let plan = match setup {
        ActiveSetupOutcome::ObservedFailure {
            observation,
            buffer,
        } => return failure::observed_failure(observation, buffer),
        ActiveSetupOutcome::Cancelled {
            observation,
            buffer,
        } => return failure::cancelled(observation, buffer),
        ActiveSetupOutcome::Ready(ready) => {
            let outbox = SubscriptionOutbox::new(
                &config.settings,
                config.bytes.clone(),
                ready.overflow,
                ready.transport,
                None,
            );
            match outbox {
                Ok(mut outbox) => {
                    outbox.attach_source_delivery(ready.deadline.source_delivery());
                    profile(
                        config,
                        ready.transport,
                        ready.deadline,
                        ready.observation,
                        ready.headers,
                        None,
                        true,
                        Some(outbox),
                    )
                }
                Err(error) => {
                    let response = ready.observation.worker_failure(config.media, &error);
                    InitialPlan {
                        deadline: ready.deadline,
                        response,
                        observation: ready.observation,
                        completion: Some(ResponseCompletionOutcome::Failed),
                        source: true,
                        outbox: None,
                        capacity_rejection: false,
                        profile: None,
                    }
                }
            }
        }
        ActiveSetupOutcome::Response {
            deadline,
            response,
            headers,
            observation,
            transport,
        } => profile(
            config,
            transport,
            deadline,
            observation,
            headers,
            Some(&response),
            true,
            None,
        ),
        ActiveSetupOutcome::CompletionResponse {
            deadline,
            response,
            outcome,
            observation,
            ..
        } => InitialPlan {
            deadline,
            response,
            observation,
            completion: Some(outcome),
            source: false,
            outbox: None,
            capacity_rejection: false,
            profile: None,
        },
        ActiveSetupOutcome::Rejected {
            deadline,
            response,
            observation,
            ..
        } => InitialPlan {
            deadline,
            response,
            observation,
            completion: None,
            source: false,
            outbox: None,
            capacity_rejection: false,
            profile: None,
        },
        ActiveSetupOutcome::TimedOut {
            observation,
            buffer,
            ..
        } => {
            return failure::timed_out(config.media, observation, buffer);
        }
        ActiveSetupOutcome::Failed {
            observation,
            buffer,
            ..
        } => {
            return failure::failed(
                config.media,
                observation,
                buffer,
                &SubscriptionWorkerRuntimeError::Shutdown,
            );
        }
    };
    finish(config, plan).await
}

#[allow(clippy::too_many_arguments)]
pub(super) fn profile<C: Send + Sync + 'static>(
    config: &HttpWorkerConfiguration,
    transport: SubscriptionTransport,
    deadline: InitialResponseDeadline,
    observation: InitialRequestObservation<C>,
    headers: ResponseHeaders,
    error: Option<&GraphQlResponse>,
    completion: bool,
    outbox: Option<SubscriptionOutbox>,
) -> InitialPlan<C> {
    let source = outbox.is_some();
    let encoded =
        __encode_subscription_http_initial(transport, error, headers, MAX_INITIAL_RESPONSE_BYTES);
    let (response, outcome, outbox, profile) = match encoded {
        Ok(response) => (
            response,
            ResponseCompletionOutcome::Encoded,
            outbox,
            Some(transport),
        ),
        Err(error) => {
            let outcome = if matches!(
                error,
                SubscriptionControlEncodingError::Json(JsonConversionError::ResponseSizeLimit)
                    | SubscriptionControlEncodingError::Event(
                        SubscriptionEventEncodingError::Json(
                            JsonConversionError::ResponseSizeLimit
                        )
                    )
            ) {
                ResponseCompletionOutcome::EncodingLimitExceeded
            } else {
                ResponseCompletionOutcome::Failed
            };
            (
                observation.worker_failure(config.media, &error),
                outcome,
                None,
                None,
            )
        }
    };
    InitialPlan {
        deadline,
        response,
        observation,
        completion: completion.then_some(outcome),
        source,
        outbox,
        capacity_rejection: false,
        profile,
    }
}

pub(super) async fn finish<C: Send + Sync + 'static>(
    config: &HttpWorkerConfiguration,
    plan: InitialPlan<C>,
) -> PreparedHttpSubscription {
    let media = config.media;
    let InitialPlan {
        mut deadline,
        mut response,
        observation,
        completion,
        source,
        outbox,
        capacity_rejection,
        profile,
    } = plan;
    let gate = poll_fn(|context| {
        let gate = match deadline.poll_before_commit(context) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(gate) => gate,
        };
        if gate == InitialResponseGate::Ready && completion.is_none() && !source {
            return deadline
                .poll_response_finished(context)
                .map(|result| match result {
                    Ok(()) => InitialResponseGate::Ready,
                    Err(_) => InitialResponseGate::Failed,
                });
        }
        Poll::Ready(gate)
    })
    .await;
    match gate {
        InitialResponseGate::Ready => {}
        InitialResponseGate::TimedOut => {
            return failure::from_deadline(media, observation, deadline, true);
        }
        InitialResponseGate::Failed => {
            return failure::from_deadline(media, observation, deadline, false);
        }
    }
    if let Some(outcome) = completion {
        if deadline.begin_completion(response, outcome).is_err() {
            return failure::from_deadline(media, observation, deadline, false);
        }
        response = match poll_fn(|context| deadline.poll_completion(context)).await {
            Ok(response) => response,
            Err(_) => return failure::from_deadline(media, observation, deadline, false),
        };
    }
    #[cfg(test)]
    config.fault.apply(&mut deadline, &config.bytes).await;
    if let Some(outbox) = outbox.filter(|_| response.status == 200) {
        let physical = match deadline.physical_lease() {
            Ok(physical) => physical,
            Err(_) => return failure::from_deadline(media, observation, deadline, false),
        };
        let (worker, charge) = match deadline.commit_source(&response) {
            Ok(committed) => committed,
            Err(_) => return failure::from_deadline(media, observation, deadline, false),
        };
        let configuration = super::output::HttpStreamConfiguration {
            bytes: config.bytes.clone(),
            transport: outbox.transport(),
            heartbeat: config.settings.limits().heartbeat_interval,
            maximum_event_bytes: config.settings.limits().max_encoded_event_bytes,
            send_no_progress: config.settings.limits().send_no_progress_timeout,
            failure: observation.stream_failure_reporter(worker.retained_telemetry()),
        };
        failure::observe(observation, response.status, capacity_rejection);
        return PreparedHttpSubscription::Ready(ReadyHttpSubscription::new(
            SubscriptionInitialResponse::charged_stream(
                response,
                charge,
                physical,
                configuration.transport,
            ),
            outbox,
            HttpSubscriptionEvents::new(worker),
            configuration,
        ));
    }
    let charge = if source {
        let charge = match deadline.finish_bytes(&response) {
            Ok(charge) => charge,
            Err(_) => return failure::from_deadline(media, observation, deadline, false),
        };
        // Failed preparation or an application-written rejection cannot emit events.
        drop(deadline);
        charge
    } else {
        match deadline.commit_response(&response) {
            Ok(charge) => charge,
            Err(_) => return failure::from_deadline(media, observation, deadline, false),
        }
    };
    failure::observe(observation, response.status, capacity_rejection);
    let response = match profile {
        Some(profile) => SubscriptionInitialResponse::charged_profile(response, charge, profile),
        None => SubscriptionInitialResponse::charged(response, charge),
    };
    PreparedHttpSubscription::Response(response)
}
