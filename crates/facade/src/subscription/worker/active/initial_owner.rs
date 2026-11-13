//! Ordered source and initial completion ownership inside the Tokio task.

use std::panic::{AssertUnwindSafe, catch_unwind};

use chigiri_http::{ResponseCompletionOutcome, ResponseParts};
use chigiri_runtime::__private::SubscriptionSession;

use crate::subscription::control::{InitialCompletionFallbacks, initial_response_charge};
use crate::subscription::driver::{EndpointSubscription, SubscriptionCompletion};
use crate::subscription::worker::completion::{dispose_payload, dispose_value};

pub(crate) struct SourceAndInitialCompletion<C: Send + Sync + 'static> {
    source: Option<TaskSource<C>>,
    completion: Option<SubscriptionCompletion>,
    fallbacks: Option<InitialCompletionFallbacks>,
    failed: bool,
    activity: Option<crate::managed::EventActivity>,
}

enum TaskSource<C: Send + Sync + 'static> {
    Setup(Box<SubscriptionSession<C>>),
    Active(Box<EndpointSubscription<C>>),
}

impl<C: Send + Sync + 'static> SourceAndInitialCompletion<C> {
    pub(super) fn retained_telemetry(&self) -> Option<&chigiri_runtime::RequestTelemetry> {
        match &self.source {
            Some(TaskSource::Active(subscription)) => subscription.retained_telemetry(),
            _ => None,
        }
    }

    pub(super) const fn initial(
        fallbacks: Option<InitialCompletionFallbacks>,
        activity: Option<crate::managed::EventActivity>,
    ) -> Self {
        Self {
            source: None,
            completion: None,
            fallbacks,
            failed: false,
            activity,
        }
    }

    pub(crate) fn arm(&mut self, completion: SubscriptionCompletion) {
        self.completion = Some(completion);
    }

    pub(crate) fn setup_source(&mut self, session: Box<SubscriptionSession<C>>) {
        self.source = Some(TaskSource::Setup(session));
    }

    pub(crate) fn take_setup_source(&mut self) -> Option<SubscriptionSession<C>> {
        match self.source.take() {
            Some(TaskSource::Setup(session)) => Some(*session),
            other => {
                self.source = other;
                None
            }
        }
    }

    pub(super) fn active_source(&mut self, subscription: Box<EndpointSubscription<C>>) {
        self.source = Some(TaskSource::Active(subscription));
    }

    pub(super) fn overflow(&self) -> Option<chigiri_runtime::SubscriptionOverflow> {
        match &self.source {
            Some(TaskSource::Active(subscription)) => subscription.overflow(),
            _ => None,
        }
    }

    pub(super) fn observation_receipt(
        &self,
    ) -> Option<chigiri_runtime::__private::SubscriptionObservationReceipt> {
        match &self.source {
            Some(TaskSource::Active(subscription)) => {
                subscription.host_observation_receipt().cloned()
            }
            _ => None,
        }
    }

    pub(crate) fn discard_source(&mut self) {
        self.drop_source();
    }

    pub(super) fn finish_normal(
        &mut self,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> Result<ResponseParts, ()> {
        let source_ok = if outcome == ResponseCompletionOutcome::Encoded {
            true
        } else {
            self.drop_source()
        };
        let completion = self.completion.take().ok_or(())?;
        let response = self.complete(completion, response, outcome)?;
        if initial_response_charge(&response).is_err() {
            self.fail();
            self.disposed(dispose_value(response));
            return Err(());
        }
        if let Some(fallbacks) = &mut self.fallbacks {
            fallbacks.clear();
        }
        if source_ok && !self.failed {
            Ok(response)
        } else {
            self.disposed(dispose_value(response));
            Err(())
        }
    }

    pub(super) fn take_source(&mut self) -> Option<Box<EndpointSubscription<C>>> {
        match self.source.take() {
            Some(TaskSource::Active(subscription)) => Some(subscription),
            other => {
                self.source = other;
                None
            }
        }
    }

    pub(super) fn finish_terminal(&mut self, outcome: ResponseCompletionOutcome) -> bool {
        let source_ok = self.drop_source();
        let completion = self.completion.take();
        let response = self
            .fallbacks
            .as_mut()
            .and_then(|fallbacks| fallbacks.take(outcome).ok());
        if let Some(fallbacks) = &mut self.fallbacks {
            fallbacks.clear();
        }
        let callback_ok = match (completion, response) {
            (Some(completion), Some(response)) => {
                match self.complete(completion, response, outcome) {
                    Ok(response) => {
                        let bounded = initial_response_charge(&response).is_ok();
                        if !bounded {
                            self.fail();
                        }
                        let disposed = self.disposed(dispose_value(response));
                        bounded && disposed
                    }
                    Err(()) => false,
                }
            }
            (Some(completion), None) if self.activity.is_some() => {
                let response = ResponseParts {
                    status: if outcome == ResponseCompletionOutcome::TimedOut {
                        504
                    } else {
                        500
                    },
                    headers: Vec::new(),
                    body: Vec::new(),
                };
                match self.complete(completion, response, outcome) {
                    Ok(response) => self.disposed(dispose_value(response)),
                    Err(()) => false,
                }
            }
            (Some(completion), None) => self.disposed(dispose_value(completion)),
            (None, Some(response)) => self.disposed(dispose_value(response)),
            (None, None) => true,
        };
        source_ok && callback_ok && !self.failed
    }

    fn drop_source(&mut self) -> bool {
        let disposed = self.source.take().is_none_or(dispose_value);
        self.disposed(disposed)
    }

    fn disposed(&mut self, disposed: bool) -> bool {
        if !disposed {
            self.failed = true;
            if let Some(activity) = &self.activity {
                activity.poison_task();
            }
        }
        disposed
    }

    fn fail(&mut self) {
        self.failed = true;
        if let Some(activity) = &self.activity {
            activity.fail_task();
        }
    }

    fn complete(
        &mut self,
        completion: SubscriptionCompletion,
        response: ResponseParts,
        outcome: ResponseCompletionOutcome,
    ) -> Result<ResponseParts, ()> {
        if let Some(activity) = &self.activity {
            let result = super::initial_completion::finish(completion, response, outcome, activity);
            if result.is_err() {
                self.failed = true;
            }
            return result;
        }
        match catch_unwind(AssertUnwindSafe(|| completion.finish(response, outcome))) {
            Ok(response) => Ok(response),
            Err(payload) => {
                let _ = dispose_payload(payload);
                Err(())
            }
        }
    }
}

impl<C: Send + Sync + 'static> Drop for SourceAndInitialCompletion<C> {
    fn drop(&mut self) {
        let _ = self.finish_terminal(ResponseCompletionOutcome::Failed);
    }
}
