//! Early direct capture and lazy request scope ownership around actual work.

use chigiri_core::executor::BoxFuture;
use chigiri_core::{DeadlineFactory, GraphQlRequest, GraphQlResponse};

use crate::ExecutableSchema;
use crate::instrumentation::{CapturedInstrumentation, InstrumentationKind, instrument_future};

impl<C: Send + Sync + 'static> ExecutableSchema<C> {
    /// Initializes live execution without replacing already-fixed request metadata.
    #[doc(hidden)]
    pub fn prepare_request_telemetry(
        &self,
        request: &mut GraphQlRequest,
        captured: Option<&CapturedInstrumentation>,
    ) {
        if request
            .request_telemetry()
            .and_then(|value| value.instrumentation_scope())
            .is_some()
        {
            return;
        }
        request.telemetry = Some(match request.telemetry.take() {
            Some(telemetry) => match captured {
                Some(captured) => captured.metadata(InstrumentationKind::Request, || {
                    captured.start(telemetry, InstrumentationKind::Request)
                }),
                None => telemetry,
            },
            None => {
                self.request_telemetry_with_capture(None, captured, InstrumentationKind::Request)
            }
        });
    }

    pub(super) fn execute_with_operation_policy<'a>(
        &'a self,
        mut request: GraphQlRequest,
        context: C,
        query_only: bool,
        direct: bool,
        deadlines: Option<&'a dyn DeadlineFactory>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        let existing = request
            .request_telemetry()
            .and_then(|value| value.instrumentation_scope())
            .cloned();
        let owns_root = direct || existing.is_none();
        let captured = existing
            .is_none()
            .then(|| self.capture_instrumentation(&()))
            .flatten();
        let handoff = existing
            .map(|scope| {
                if direct {
                    scope
                } else {
                    scope.child(
                        InstrumentationKind::Execution,
                        chigiri_core::SamplingDecision::Drop,
                    )
                }
            })
            .or_else(|| {
                captured
                    .as_ref()
                    .map(|value| value.handoff_scope(InstrumentationKind::Request))
            });
        if handoff.is_none() {
            return Box::pin(self.execute_with_operation_policy_inner(
                request, context, query_only, direct, deadlines,
            ));
        }
        instrument_future(handoff, async move {
            self.prepare_request_telemetry(&mut request, captured.as_ref());
            let scope = request
                .request_telemetry()
                .and_then(|value| value.instrumentation_scope())
                .cloned();
            let activation = owns_root.then(|| scope.clone()).flatten();
            instrument_future(activation, async move {
                let response = self
                    .execute_with_operation_policy_inner(
                        request, context, query_only, direct, deadlines,
                    )
                    .await;
                if owns_root && let Some(scope) = scope {
                    scope.finish(super::lifecycle_result(&response));
                }
                response
            })
            .await
        })
    }
}
