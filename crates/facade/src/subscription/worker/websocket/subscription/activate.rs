//! Exact-kind active admission and source-worker construction.

use chigiri_runtime::__private::WebSocketOperationKind;

use super::{ActiveWebSocketSubscription, WebSocketSubscriptionSetup, task};
use crate::subscription::bridge_resources::{BridgeOperationKind, BridgeResourceError};
use crate::subscription::worker::port::channel;
use crate::subscription::worker::websocket::{
    PreparedWebSocketWorkerOperation, WebSocketActiveStartFailure,
};

impl<C: Send + Sync + 'static> PreparedWebSocketWorkerOperation<C> {
    pub(crate) fn activate_subscription(
        self,
    ) -> Result<WebSocketSubscriptionSetup, Box<WebSocketActiveStartFailure>> {
        let Self {
            validated,
            operation,
            deadlines,
            mut failure,
        } = self;
        if validated.prepared.kind() != WebSocketOperationKind::Subscription {
            return Err(Box::new(WebSocketActiveStartFailure::pending_completed(
                BridgeResourceError::InvalidOperationKind,
                operation,
                deadlines,
                None,
                failure,
            )));
        }
        let (deadlines, armer, factory) = deadlines.into_parts();
        let generation = operation.generation();
        let operation = match operation.activate(BridgeOperationKind::Subscription) {
            Ok(operation) => operation,
            Err(error) => {
                return Err(Box::new(WebSocketActiveStartFailure::completed(
                    error, generation, deadlines, None, failure,
                )));
            }
        };
        let (setup_sender, setup_receiver) = channel();
        let (demand_sender, demand_receiver) = channel();
        let (event_sender, event_receiver) = channel();
        let (activity, reporter) = validated.state.task_scope();
        let captured = validated
            .request
            .request()
            .request_telemetry()
            .and_then(chigiri_runtime::RequestTelemetry::propagation_context)
            .and_then(|values| validated.state.schema.instrumentation_with_values(values));
        let instrumentation = captured.as_ref().map(|captured| {
            captured.retain_within(0).handoff_scope(
                chigiri_runtime::instrumentation::InstrumentationKind::SubscriptionSetup,
            )
        });
        let armer = armer
            .with_task(activity.clone())
            .with_instrumentation(captured.map(|captured| captured.retain_within(0)));
        if activity.is_managed() {
            failure.bind_task(activity.clone());
        }
        let work = task::run(
            validated,
            armer,
            setup_sender,
            demand_receiver,
            event_sender,
            activity.clone(),
        );
        let spawned = match reporter {
            Some(reporter) => operation.spawn_worker(
                crate::subscription::worker::managed_task::ManagedSubscriptionTask::new(
                    work,
                    activity.clone(),
                    reporter,
                )
                .with_instrumentation(instrumentation),
            ),
            None => operation.spawn_worker(chigiri_runtime::instrumentation::instrument_future(
                instrumentation,
                work,
            )),
        };
        let mut worker = match spawned {
            Ok(worker) => worker,
            Err(error) => {
                return Err(Box::new(WebSocketActiveStartFailure::active(
                    error, operation, deadlines, None, failure, factory,
                )));
            }
        };
        if activity.is_managed() {
            worker.retain_subscription_integrity(activity.clone());
        }
        Ok(WebSocketSubscriptionSetup {
            subscription: Some(ActiveWebSocketSubscription {
                activity,
                observation: None,
                operation: Some(operation),
                worker: Some(worker),
                deadlines: Some(deadlines),
                deadline_factory: factory,
                demand_sender,
                event_receiver,
                reservation: None,
                terminal_event_seen: false,
                failure: Some(failure),
            }),
            setup_receiver,
        })
    }
}
