//! Required host data takes priority before its allocation and outside handoff.

use std::sync::Arc;

use chigiri_runtime::__private::PreparedSubscription;
use chigiri_runtime::GraphQlRequest;

use super::{MetadataRetentionLimit, RetentionError, SubscriberRequest};

impl SubscriberRequest {
    pub(crate) fn reserve_host_metadata<C>(
        &mut self,
        prepared: &mut PreparedSubscription<C>,
        limit: usize,
    ) -> Result<usize, RetentionError> {
        let required = self
            .charged_bytes
            .checked_sub(self.optional_bytes()?)
            .ok_or(RetentionError::Overflow)?;
        let available = MetadataRetentionLimit::new(limit)?.available(required)?;
        // Release optional source retention before the host may allocate. The
        // short setup telemetry remains an independent execution owner.
        self.retain_budget(prepared, 0)?;
        self.charged_bytes = required;
        Ok(available)
    }

    pub(crate) fn charge_host_observation<C>(
        &mut self,
        prepared: &mut PreparedSubscription<C>,
        bytes: usize,
        limit: usize,
    ) -> Result<(), RetentionError> {
        let total = self
            .charged_bytes
            .checked_add(bytes)
            .ok_or(RetentionError::Overflow)?;
        let available = MetadataRetentionLimit::new(limit)?.available(total)?;
        self.retain_budget(prepared, available)?;
        self.charged_bytes = total
            .checked_add(self.optional_bytes()?)
            .ok_or(RetentionError::Overflow)?;
        Ok(())
    }

    fn optional_bytes(&self) -> Result<usize, RetentionError> {
        let Some(telemetry) = self.request().request_telemetry() else {
            return Ok(0);
        };
        let Some(values) = telemetry.propagation_context() else {
            return Ok(0);
        };
        values
            .correlation_bytes()
            .checked_add(telemetry.trace_id().map_or(0, |id| id.as_str().len()))
            .ok_or(RetentionError::Overflow)
    }

    fn retain_budget<C>(
        &mut self,
        prepared: &mut PreparedSubscription<C>,
        available: usize,
    ) -> Result<(), RetentionError> {
        // The two SDK owners share one request allocation. Temporarily release
        // this handle, without await or payload copying, to obtain uniqueness.
        drop(self.request.take().ok_or(RetentionError::Overflow)?);
        let share = Reattach {
            request: &mut self.request,
            prepared,
        };
        let result = share.prepared.retain_instrumentation(available);
        drop(share);
        result.ok_or(RetentionError::Overflow)
    }
}

/// Reattaches even if a custom instrumentation callback unwinds during resize.
struct Reattach<'a, C> {
    request: &'a mut Option<Arc<GraphQlRequest>>,
    prepared: &'a mut PreparedSubscription<C>,
}

impl<C> Drop for Reattach<'_, C> {
    fn drop(&mut self) {
        *self.request = Some(self.prepared.retained_request());
    }
}
