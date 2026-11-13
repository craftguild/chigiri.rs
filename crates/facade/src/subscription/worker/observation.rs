//! Transfer the sole closed host owner through an existing capacity-one port.

use chigiri_runtime::__private::{PreparedSubscription, SubscriptionObservation};
use chigiri_runtime::{GraphQlError, GraphQlResponse};

use crate::subscription::request::SubscriberRequest;

pub(crate) trait HostObservationHandoff: Sync {
    fn transfer(&self, observation: SubscriptionObservation)
    -> Result<(), SubscriptionObservation>;
}

pub(crate) fn transfer<C: Send + Sync + 'static>(
    prepared: &mut PreparedSubscription<C>,
    context: &C,
    request: &mut SubscriberRequest,
    metadata_limit: usize,
    handoff: &dyn HostObservationHandoff,
) -> Result<(), Box<GraphQlResponse>> {
    let remaining = request
        .reserve_host_metadata(prepared, metadata_limit)
        .map_err(|_| capacity())?;
    let observation = prepared.prepare_host_observation(context, remaining);
    let bytes = observation
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map_or(0, SubscriptionObservation::retained_metadata_bytes);
    request
        .charge_host_observation(prepared, bytes, metadata_limit)
        .map_err(|_| capacity())?;
    let Some(observation) = observation? else {
        return Ok(());
    };
    handoff.transfer(observation).map_err(|observation| {
        // Entry is still impossible: start waits for outside acknowledgement.
        // Drop closes the receipt and cannot create a dispatch-entered record.
        drop(observation);
        Box::new(GraphQlResponse::request_error(
            GraphQlError::new("Internal server error").code("INTERNAL_SERVER_ERROR"),
        ))
    })
}

fn capacity() -> Box<GraphQlResponse> {
    Box::new(crate::subscription::driver::input::metadata_graphql_response())
}
