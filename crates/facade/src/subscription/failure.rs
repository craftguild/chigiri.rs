//! Shared safe subscription failure mapping with the original diagnostic source.

use std::error::Error;
use std::sync::Arc;

use chigiri_runtime::{
    ExecutableSchema, GraphQlError, GraphQlResponse, LifecyclePhase, RequestTelemetry,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::assert_shared_ids;

/// Closed response class selected by common delivery and worker policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SubscriptionFailureClass {
    TimedOut,
    Limit,
    Internal,
}

/// Request-bound failure mapping shared by HTTP and WebSocket supervision.
#[derive(Clone)]
pub(crate) struct SubscriptionFailureReporter {
    response: Arc<FailureResponse>,
    telemetry: RequestTelemetry,
    activity: Option<crate::managed::EventActivity>,
}

type FailureSource = dyn Error + Send + Sync + 'static;
type FailureResponse = dyn Fn(&RequestTelemetry, SubscriptionFailureClass, &FailureSource) -> GraphQlResponse
    + Send
    + Sync;

impl SubscriptionFailureReporter {
    pub(crate) fn new<C: Send + Sync + 'static>(
        schema: ExecutableSchema<C>,
        telemetry: RequestTelemetry,
        component: &'static str,
    ) -> Self {
        Self {
            activity: None,
            // Before exact admission, only fixed control is safe to retain.
            telemetry: telemetry.retained_instrumentation(0),
            response: Arc::new(move |telemetry, class, source| match class {
                SubscriptionFailureClass::TimedOut => crate::timeout::request_timeout_response(),
                SubscriptionFailureClass::Limit => GraphQlResponse::request_error(
                    GraphQlError::new("Subscription delivery limit exceeded")
                        .code("SUBSCRIPTION_LIMIT_EXCEEDED"),
                ),
                SubscriptionFailureClass::Internal => {
                    let captured = telemetry
                        .propagation_context()
                        .and_then(|values| schema.instrumentation_with_values(values));
                    let instrumentation = captured.map(|captured| {
                        captured.handoff_scope(
                            chigiri_runtime::instrumentation::InstrumentationKind::Provider {
                                phase: LifecyclePhase::Execution,
                                component,
                            },
                        )
                    });
                    let bound = instrumentation
                        .as_ref()
                        .map(|scope| telemetry.clone().with_instrumentation_scope(scope.clone()));
                    let telemetry = bound.as_ref().unwrap_or(telemetry);
                    let observe = || {
                        let id = schema.observe_internal_error(
                            LifecyclePhase::Execution,
                            "INTERNAL_SERVER_ERROR",
                            source,
                            Some(telemetry),
                            component,
                        );
                        let mut error = GraphQlError::new("Internal server error")
                            .code("INTERNAL_SERVER_ERROR");
                        if let Some(id) = id {
                            error = error.extension("errorId", id.as_str());
                        }
                        GraphQlResponse::request_error(error)
                    };
                    match instrumentation {
                        Some(scope) => scope.run(observe),
                        None => observe(),
                    }
                }
            }),
        }
    }

    pub(crate) fn response(
        &self,
        class: SubscriptionFailureClass,
        source: &FailureSource,
    ) -> GraphQlResponse {
        if class == SubscriptionFailureClass::Internal
            && self
                .activity
                .as_ref()
                .is_some_and(crate::managed::EventActivity::task_failed)
        {
            return GraphQlResponse::request_error(
                GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR"),
            );
        }
        (self.response)(&self.telemetry, class, source)
    }

    /// Receives the final detached snapshot after exact admission/host charges.
    pub(crate) fn bind_retained_telemetry(&mut self, telemetry: Option<&RequestTelemetry>) {
        if let Some(telemetry) = telemetry.filter(|value| value.instrumentation_scope().is_none()) {
            self.telemetry = telemetry.clone();
            #[cfg(test)]
            assert_shared_ids(Some(telemetry), Some(&self.telemetry));
        }
    }

    pub(crate) fn bind_task(&mut self, activity: crate::managed::EventActivity) {
        self.activity = Some(activity);
    }
}

pub(crate) const fn outbox_failure_class(
    signal: crate::subscription::outbox::SubscriptionOutboxSignal,
) -> SubscriptionFailureClass {
    use crate::subscription::outbox::SubscriptionOutboxSignal;

    match signal {
        SubscriptionOutboxSignal::CounterExhausted
        | SubscriptionOutboxSignal::OversizedEvent
        | SubscriptionOutboxSignal::QueueAgeExpired
        | SubscriptionOutboxSignal::SendStalled
        | SubscriptionOutboxSignal::SharedBudgetPressureExpired
        | SubscriptionOutboxSignal::CapacityRejected => SubscriptionFailureClass::Limit,
        _ => SubscriptionFailureClass::Internal,
    }
}

pub(crate) const fn control_failure_class(
    signal: crate::subscription::control::SubscriptionControlSignal,
) -> SubscriptionFailureClass {
    use crate::subscription::control::SubscriptionControlSignal;

    match signal {
        SubscriptionControlSignal::SharedByteCapacity | SubscriptionControlSignal::ControlLimit => {
            SubscriptionFailureClass::Limit
        }
        _ => SubscriptionFailureClass::Internal,
    }
}

/// Classifies typed common policy failures without inspecting response content.
pub(crate) const fn outbox_termination(
    signal: crate::subscription::outbox::SubscriptionOutboxSignal,
) -> chigiri_runtime::__private::SubscriptionTermination {
    use crate::subscription::outbox::SubscriptionOutboxSignal;
    use chigiri_runtime::__private::SubscriptionTermination;
    match signal {
        SubscriptionOutboxSignal::Encoding
        | SubscriptionOutboxSignal::DeliveryExtensionCollision => {
            return SubscriptionTermination::DeliveryFailed;
        }
        _ => {}
    }
    match outbox_failure_class(signal) {
        SubscriptionFailureClass::Limit => SubscriptionTermination::DeliveryFailed,
        _ => SubscriptionTermination::Internal,
    }
}

pub(crate) const fn control_termination(
    signal: crate::subscription::control::SubscriptionControlSignal,
) -> chigiri_runtime::__private::SubscriptionTermination {
    use chigiri_runtime::__private::SubscriptionTermination;
    match control_failure_class(signal) {
        SubscriptionFailureClass::Limit => SubscriptionTermination::DeliveryFailed,
        _ => SubscriptionTermination::Internal,
    }
}
