//! HTTP classification delegated to the shared safe subscription mapper.

use chigiri_runtime::{ExecutableSchema, GraphQlResponse, RequestTelemetry};

use super::HttpStreamingError;
use crate::subscription::failure::{
    SubscriptionFailureClass, SubscriptionFailureReporter, control_failure_class,
    outbox_failure_class,
};

pub(crate) struct StreamFailureReporter {
    shared: SubscriptionFailureReporter,
    activity: Option<crate::managed::EventActivity>,
}

impl StreamFailureReporter {
    pub(crate) fn bind_retained_telemetry(&mut self, telemetry: Option<&RequestTelemetry>) {
        self.shared.bind_retained_telemetry(telemetry);
    }

    pub(crate) fn new<C: Send + Sync + 'static>(
        schema: ExecutableSchema<C>,
        telemetry: RequestTelemetry,
    ) -> Self {
        Self {
            activity: None,
            shared: SubscriptionFailureReporter::new(
                schema,
                telemetry,
                "subscription-http-supervision",
            ),
        }
    }

    pub(super) fn bind_task(&mut self, activity: crate::managed::EventActivity) {
        self.activity = Some(activity);
    }

    pub(super) fn response(&self, error: HttpStreamingError) -> GraphQlResponse {
        let class = match error {
            HttpStreamingError::TimedOut => SubscriptionFailureClass::TimedOut,
            HttpStreamingError::Delivery(signal) => outbox_failure_class(signal),
            HttpStreamingError::Control(signal) => control_failure_class(signal),
            _ => SubscriptionFailureClass::Internal,
        };
        if class == SubscriptionFailureClass::Internal
            && self
                .activity
                .as_ref()
                .is_some_and(crate::managed::EventActivity::task_failed)
        {
            return GraphQlResponse::request_error(
                chigiri_runtime::GraphQlError::new("Internal server error")
                    .code("INTERNAL_SERVER_ERROR"),
            );
        }
        self.shared.response(class, &error)
    }
}
