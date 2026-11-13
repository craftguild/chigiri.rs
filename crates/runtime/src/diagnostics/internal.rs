//! Maps executor-internal errors to the shared observer representation.

use chigiri_core::executor::InternalErrorHandler;
use chigiri_core::{
    ErrorEvent, FieldError, LifecyclePhase, OperationIdentity, PathSegment, RequestIdentity,
    RequestTelemetry,
};

use super::Diagnostics;

impl InternalErrorHandler for Diagnostics {
    fn handle_internal_error(
        &self,
        error: &mut FieldError,
        identity: &OperationIdentity,
        telemetry: Option<&RequestTelemetry>,
        path: &[PathSegment],
        coordinate: Option<&str>,
        component: &'static str,
    ) {
        if error.error_id().is_none()
            && let Some(error_id) =
                self.create_error_id(LifecyclePhase::Execution, Some(identity), telemetry)
        {
            error.assign_error_id(error_id);
        }
        if let Some(source) = error.internal_source() {
            self.notify_error(
                ErrorEvent::new(
                    error.error_id(),
                    telemetry.map_or_else(
                        || RequestIdentity::new(None, None, Some(identity)),
                        |telemetry| telemetry.identity(Some(identity)),
                    ),
                    LifecyclePhase::Execution,
                    "INTERNAL_SERVER_ERROR",
                    source,
                    Some(path),
                    coordinate,
                    component,
                ),
                telemetry,
            );
        }
    }
}
