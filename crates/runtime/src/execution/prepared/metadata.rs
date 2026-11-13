//! Checked charge shared by deferred prepared-operation owners.

use chigiri_core::GraphQlRequest;
use chigiri_core::executor::SubscriptionMetadataCharge;

pub(crate) fn retained_request_metadata_bytes(
    core_bytes: usize,
    request: &GraphQlRequest,
) -> Option<usize> {
    let mut charge = SubscriptionMetadataCharge::new();
    charge.add_bytes(core_bytes)?;
    charge.add_bytes(request.query.len())?;
    if let Some(name) = &request.operation_name {
        charge.add_bytes(name.len())?;
    }
    charge.object(&request.variables)?;
    charge.object(&request.extensions)?;
    if let Some(telemetry) = request.request_telemetry() {
        if let Some(id) = telemetry.request_id() {
            charge.add_bytes(id.as_str().len())?;
        }
        if let Some(id) = telemetry.trace_id() {
            charge.add_bytes(id.as_str().len())?;
        }
    }
    Some(charge.bytes())
}
