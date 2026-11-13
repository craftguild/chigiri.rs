//! Mint one opaque owner only through the validated dispatch preparation seam.

use chigiri_core::FieldResult;
use chigiri_core::executor::InternalErrorHandler;

use super::{SubscriptionObservation, SubscriptionObservationReceipt};
use crate::subscription::PreparedSubscription;

impl<C: Send + Sync + 'static> PreparedSubscription<C> {
    /// Prepares a closed optional host owner after fresh context creation.
    ///
    /// The caller transfers it through the existing capacity-one setup handoff
    /// and acknowledges outside ownership before invoking `start`. Ordinary
    /// generated dispatch returns None and receives no Mock identity or sink.
    #[doc(hidden)]
    pub fn prepare_host_observation(
        &mut self,
        context: &C,
        remaining_metadata_bytes: usize,
    ) -> Result<Option<SubscriptionObservation>, Box<chigiri_core::GraphQlResponse>> {
        self.prepare_host_observation_inner(context, remaining_metadata_bytes)
            .map_err(|mut error| {
                let path = vec![self.plan.response_key().into()];
                let coordinate = format!("{}.{}", self.plan.parent_type(), self.plan.field_name());
                if error.is_internal() {
                    self.schema.inner.diagnostics.handle_internal_error(
                        &mut error,
                        &self.identity,
                        self.setup_telemetry(),
                        &path,
                        Some(&coordinate),
                        "subscription-observation",
                    );
                }
                Box::new(chigiri_core::GraphQlResponse::request_error(
                    error.to_graphql_error(self.plan.locations().to_vec(), path),
                ))
            })
    }

    fn prepare_host_observation_inner(
        &mut self,
        context: &C,
        remaining_metadata_bytes: usize,
    ) -> FieldResult<Option<SubscriptionObservation>> {
        if self.observation_prepared {
            return Err(crate::private::invariant_error(
                "subscription host observation was already prepared",
            ));
        }
        self.observation_prepared = true;
        let Some(dispatch) = self.schema.inner.subscription.as_deref() else {
            return Ok(None);
        };
        let receipt = SubscriptionObservationReceipt::new();
        self.observation = Some(receipt.clone());
        let fixed = std::mem::size_of::<SubscriptionObservation>()
            .checked_add(SubscriptionObservationReceipt::retained_bytes())
            // Receipt handles in setup/source/session and the one deferred
            // source slot are fixed host plumbing, never uncharged metadata.
            .and_then(|bytes| {
                bytes.checked_add(3 * std::mem::size_of::<SubscriptionObservationReceipt>())
            })
            .and_then(|bytes| {
                bytes.checked_add(std::mem::size_of::<
                    Option<crate::subscription::erased::EventStream>,
                >())
            })
            .and_then(|bytes| bytes.checked_add(2 * std::mem::size_of::<bool>()))
            .ok_or_else(metadata_error)?;
        let host = match dispatch.prepare_observation(
            context,
            self.plan.parent_type(),
            self.plan.field_name(),
            receipt.clone(),
            remaining_metadata_bytes.saturating_sub(fixed),
        ) {
            Ok(host) => host,
            Err(error) => {
                receipt.close();
                return Err(error);
            }
        };
        let Some(host) = host else {
            receipt.close();
            self.observation = None;
            return Ok(None);
        };
        let Some(retained_bytes) = fixed
            .checked_add(host.retained_metadata_bytes())
            .filter(|bytes| *bytes <= remaining_metadata_bytes)
        else {
            receipt.close();
            return Err(metadata_error());
        };
        Ok(Some(SubscriptionObservation {
            host: Some(host),
            receipt,
            retained_bytes,
            dropped_total: 0,
        }))
    }
}

fn metadata_error() -> chigiri_core::FieldError {
    crate::private::invariant_error("subscription observation metadata exceeds its bound")
}
