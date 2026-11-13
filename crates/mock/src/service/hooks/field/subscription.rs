//! Consume source setup and publication projection within the ordinary hook frame.

use std::sync::Arc;

use super::{FIELD, Frame, diagnosed, diagnostic, error};
use crate::ir::FieldId;
use crate::luau::ScriptError;
use crate::output::OutputValue;
use crate::publication::CapturedEvent;
use crate::service::Client;
use crate::value::ValueBudget;

#[cfg(test)]
tokio::task_local! { static EXHAUST_PROJECTION: (); }

#[cfg(test)]
pub(in crate::service) async fn exhaust_projection_for_test<T>(
    future: impl std::future::Future<Output = T>,
) -> T {
    EXHAUST_PROJECTION.scope((), future).await
}

fn frame(field: FieldId) -> Result<Arc<Frame>, ScriptError> {
    FIELD
        .try_with(Arc::clone)
        .map_err(|_| error("subscription hook frame required"))
        .and_then(|frame| {
            if frame.field == field {
                Ok(frame)
            } else {
                Err(error("subscription field differs from hook frame"))
            }
        })
}

pub(in crate::service) async fn establish_subscription(
    client: &Client,
    field: FieldId,
    arguments: chigiri::Object,
) -> Result<super::super::EstablishedSubscription, ScriptError> {
    let frame = frame(field)?;
    let client = frame.session.client.get().unwrap_or(client);
    let mut execution = frame.session.execution(client)?;
    execution.budget = execution.budget.at(frame.origin.clone());
    let client = diagnosed(client, &frame);
    let observation = super::super::subscription_observation::enter(&frame.session.trace)?;
    let result = client
        .establish_subscription(
            field,
            arguments,
            frame.before.clone(),
            execution,
            observation,
        )
        .await;
    if let Err(error) = &result {
        diagnostic(&client, &frame, error);
    }
    *frame.result.lock().expect("setup result generation") = frame.before.clone();
    result
}

pub(in crate::service) fn project_event(
    client: &Client,
    field: FieldId,
    event: &CapturedEvent,
) -> Result<OutputValue, ScriptError> {
    let frame = frame(field)?;
    let client = frame.session.client.get().unwrap_or(client);
    let result = (|| {
        client.validate_subscription_event(event)?;
        if let Some(cancel) = event.cancellation() {
            frame.session.source_cancel.get_or_init(|| cancel.clone());
        }
        let execution = frame.session.execution(client)?;
        let snapshot = frame
            .before
            .clone()
            .ok_or_else(|| error("event snapshot required"))?;
        let mut budget = ValueBudget::with_work(execution.budget.0.at(frame.origin.clone()));
        #[cfg(test)]
        if EXHAUST_PROJECTION.try_with(|_| ()).is_ok() {
            // Simulate work already spent by earlier event phases. Projection
            // still runs normally and must reject its first growth operation.
            let _ = budget.work.clone().charge(
                crate::limits::request::policy(crate::limits::request::LimitKind::Work).hard,
            );
        }
        event
            .project(snapshot, field, &mut budget)
            .map_err(|error| ScriptError::new("output", error.0))
    })();
    if let Err(error) = &result {
        diagnostic(client, &frame, error);
    }
    *frame.result.lock().expect("event result generation") = frame.before.clone();
    result
}
