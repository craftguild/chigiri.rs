//! Checked charges for the runtime-owned portion of subscriber retention.

use std::sync::Arc;

use chigiri_core::{GraphQlRequest, OperationIdentity};

use crate::execution::retained_request_metadata_bytes;

use super::PreparedSubscription;

impl<C> PreparedSubscription<C> {
    /// Borrows short setup execution metadata independently of retention pressure.
    #[doc(hidden)]
    pub fn setup_telemetry(&self) -> Option<&chigiri_core::RequestTelemetry> {
        self.setup_telemetry
            .as_deref()
            .or_else(|| self.request.request_telemetry())
    }
    /// Shares the decoded request without duplicating its retained allocation.
    #[doc(hidden)]
    pub fn retained_request(&self) -> Arc<GraphQlRequest> {
        Arc::clone(&self.request)
    }

    /// Shares the identity allocation already charged by the immutable core plan.
    #[doc(hidden)]
    pub fn retained_identity(&self) -> Arc<OperationIdentity> {
        Arc::clone(&self.identity)
    }

    /// Charges every retained runtime/core string, byte value, and collection entry.
    ///
    /// The facade adds HTTP metadata and authorized initialization data. Shared
    /// request and identity handles do not allocate another copy of their values.
    #[doc(hidden)]
    pub fn retained_metadata_bytes(&self) -> Option<usize> {
        let bytes =
            retained_request_metadata_bytes(self.plan.retained_metadata_bytes()?, &self.request)?;
        bytes.checked_add(
            self.request
                .request_telemetry()
                .and_then(chigiri_core::RequestTelemetry::propagation_context)
                .map_or(0, |values| values.correlation_bytes()),
        )
    }

    /// Returns the required charge before optional instrumentation correlation.
    #[doc(hidden)]
    pub fn required_retained_metadata_bytes(&self) -> Option<usize> {
        let bytes = self.retained_metadata_bytes()?;
        bytes.checked_sub(self.optional_instrumentation_bytes())
    }

    /// Budgets detached metadata before the facade shares the retained request.
    ///
    /// Returns None if changing metadata requires mutation after sharing. No request
    /// payload is cloned to obtain mutation.
    /// The short setup telemetry remains separate from the retained request.
    #[doc(hidden)]
    pub fn retain_instrumentation(&mut self, available: usize) -> Option<()> {
        let Some(telemetry) = self.setup_telemetry() else {
            return Some(());
        };
        if telemetry.propagation_context().is_none() {
            return Some(());
        }
        let retained = telemetry.retained_instrumentation(available);
        if self.request.request_telemetry() == Some(&retained)
            && self
                .request
                .request_telemetry()
                .and_then(chigiri_core::RequestTelemetry::propagation_context)
                == retained.propagation_context()
        {
            return Some(());
        }
        Arc::get_mut(&mut self.request)?.telemetry = Some(retained);
        Some(())
    }

    fn optional_instrumentation_bytes(&self) -> usize {
        let Some(telemetry) = self.request.request_telemetry() else {
            return 0;
        };
        let Some(values) = telemetry.propagation_context() else {
            return 0;
        };
        values.correlation_bytes() + telemetry.trace_id().map_or(0, |id| id.as_str().len())
    }
}
