//! Closed sequential HTTP preparation through the official concrete workers.

mod event;
mod failure;
mod initial;
mod output;
#[cfg(test)]
mod test_fault;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use chigiri_http::{BoxFuture, ResponseMediaType};

use super::PendingHttpSubscription;
use crate::subscription::control::{SubscriptionControlSignal, reserve_initial_preparation};
use crate::subscription::delivery::EndpointByteOwner;
use crate::subscription::driver::SubscriptionInitialization;
use crate::subscription::settings::ValidatedSubscriptionSettings;
use crate::subscription::worker::{
    ActiveSetupFailureKind, InitialWorkerResources, PendingPreparationOutcome,
    PendingPreparationWorker, SubscriptionWorkerRuntimeError,
};
use crate::transport::response::error_response;

pub use event::{HttpSubscriptionEvents, HttpSubscriptionSignal};
pub(crate) use output::HttpStreamConfiguration;
pub use output::{PreparedHttpSubscription, ReadyHttpSubscription, SubscriptionInitialResponse};
#[cfg(test)]
pub(crate) use test_fault::{InitialCommitTestBarrier, InitialCommitTestFault};
#[cfg(test)]
pub(crate) use test_fault::{InitialWorkerProbe, InitialWorkerStage};

pub(super) struct HttpWorkerConfiguration {
    media: ResponseMediaType,
    bytes: EndpointByteOwner,
    settings: ValidatedSubscriptionSettings,
    #[cfg(test)]
    fault: InitialCommitTestFault,
}

impl HttpWorkerConfiguration {
    pub(super) const fn new(
        media: ResponseMediaType,
        bytes: EndpointByteOwner,
        settings: ValidatedSubscriptionSettings,
    ) -> Self {
        Self {
            media,
            bytes,
            settings,
            #[cfg(test)]
            fault: InitialCommitTestFault::None,
        }
    }
}

/// One closed preparation future; callers cannot inject arbitrary task cleanup.
#[must_use = "HTTP subscription preparation must be polled or explicitly abandoned"]
#[doc(hidden)]
pub struct HttpSubscriptionPreparation {
    inner: BoxFuture<'static, PreparedHttpSubscription>,
}

impl Future for HttpSubscriptionPreparation {
    type Output = PreparedHttpSubscription;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(context)
    }
}

impl<C: Send + Sync + 'static> PendingHttpSubscription<'static, C> {
    #[cfg(test)]
    pub(crate) fn with_commit_fault(mut self, fault: InitialCommitTestFault) -> Self {
        self.worker_configuration.fault = fault;
        self
    }

    /// Starts bounded validation and setup using the explicit runtime registration.
    ///
    /// No source or armed completion escapes before bounded initial commit. Drop
    /// transfers already started work and its charges into generation cleanup.
    pub fn prepare(self) -> HttpSubscriptionPreparation {
        HttpSubscriptionPreparation {
            inner: Box::pin(prepare(self)),
        }
    }
}

async fn prepare<C: Send + Sync + 'static>(
    pending: PendingHttpSubscription<'static, C>,
) -> PreparedHttpSubscription {
    let PendingHttpSubscription {
        preparation,
        operation,
        observation,
        profile,
        worker_configuration: config,
    } = pending;
    let (inputs, guards) = match reserve_initial_preparation(&config.bytes, config.media) {
        Ok(bundle) => bundle,
        Err(SubscriptionControlSignal::SharedByteCapacity) => {
            drop((preparation, operation));
            return failure::admission(capacity_response(config.media), observation);
        }
        Err(_) => {
            drop((preparation, operation));
            return failure::closed(observation);
        }
    };
    let worker = match PendingPreparationWorker::new(
        preparation,
        operation,
        observation,
        profile,
        Some(InitialWorkerResources::new(inputs, guards)),
    ) {
        Ok(worker) => worker,
        Err(failure) => {
            if let Some(deadline) = failure.deadline {
                let response = failure
                    .observation
                    .worker_failure(config.media, &SubscriptionWorkerRuntimeError::Startup);
                return initial::finish(
                    &config,
                    initial::InitialPlan {
                        deadline,
                        response,
                        observation: failure.observation,
                        completion: None,
                        source: false,
                        outbox: None,
                        capacity_rejection: false,
                        profile: None,
                    },
                )
                .await;
            }
            return failure::failed(
                config.media,
                failure.observation,
                failure.buffer,
                &SubscriptionWorkerRuntimeError::Startup,
            );
        }
    };
    #[cfg(test)]
    config.fault.capture_worker(
        worker.abort_handle_for_test(),
        InitialWorkerStage::Validation,
    );
    match worker.await {
        PendingPreparationOutcome::ObservedFailure {
            observation,
            buffer,
        } => failure::observed_failure(observation, buffer),
        PendingPreparationOutcome::Cancelled {
            observation,
            buffer,
        } => failure::cancelled(observation, buffer),
        PendingPreparationOutcome::Prepared(prepared) => match (*prepared).activate() {
            Ok(setup) => {
                #[cfg(test)]
                config
                    .fault
                    .capture_worker(setup.abort_handle_for_test(), InitialWorkerStage::Setup);
                initial::active(&config, setup.await).await
            }
            Err(failure) => {
                let response = if failure.kind == ActiveSetupFailureKind::Capacity {
                    capacity_response(config.media)
                } else {
                    failure
                        .observation
                        .worker_failure(config.media, &SubscriptionWorkerRuntimeError::Invariant)
                };
                if let Some(deadline) = failure.deadline {
                    initial::finish(
                        &config,
                        initial::InitialPlan {
                            deadline,
                            response,
                            observation: failure.observation,
                            completion: None,
                            source: false,
                            outbox: None,
                            capacity_rejection: failure.kind == ActiveSetupFailureKind::Capacity,
                            profile: None,
                        },
                    )
                    .await
                } else {
                    failure::finite(
                        config.media,
                        response,
                        failure.observation,
                        failure.buffer,
                        failure.kind == ActiveSetupFailureKind::Capacity,
                    )
                }
            }
        },
        PendingPreparationOutcome::Response {
            deadline,
            initialization,
            observation,
            ..
        } => {
            // Phase A is source/context-free; its only error constructor is Response.
            match *initialization {
                SubscriptionInitialization::Response {
                    response,
                    headers,
                    completion,
                } => {
                    drop(completion);
                    let plan = initial::profile(
                        &config,
                        profile,
                        *deadline,
                        observation,
                        headers,
                        Some(&response),
                        false,
                        None,
                    );
                    initial::finish(&config, plan).await
                }
                _ => failure::from_deadline(config.media, observation, *deadline, false),
            }
        }
        PendingPreparationOutcome::TimedOut {
            observation,
            buffer,
            ..
        } => failure::timed_out(config.media, observation, buffer),
        PendingPreparationOutcome::Failed {
            observation,
            buffer,
            ..
        } => failure::failed(
            config.media,
            observation,
            buffer,
            &SubscriptionWorkerRuntimeError::Shutdown,
        ),
    }
}

fn capacity_response(media: ResponseMediaType) -> chigiri_http::ResponseParts {
    error_response(
        media,
        503,
        "Subscription capacity is unavailable",
        "SUBSCRIPTION_LIMIT_EXCEEDED",
    )
}
