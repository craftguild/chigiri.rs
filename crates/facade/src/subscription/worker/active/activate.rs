//! Transition from joined validation into the single active source worker.

use super::initial_command::initial_response_channel;
use super::{ActiveSubscriptionWorker, ActiveWorkerSetup, InitialResponseDeadline};
use crate::subscription::bridge_resources::{BridgeOperationKind, BridgeResourceError};
use crate::subscription::worker::InitialWorkerResources;
use crate::subscription::worker::pending::PreparedWorkerSubscription;
use crate::subscription::worker::port::channel;

impl<C: Send + Sync + 'static> PreparedWorkerSubscription<C> {
    pub(crate) fn activate(
        self,
    ) -> Result<ActiveWorkerSetup<C>, Box<super::ActiveSetupFailure<C>>> {
        let Self {
            prepared,
            operation,
            observation,
            transport,
            mut initial,
        } = self;
        let (validated, timers) = prepared.into_worker_parts();
        let deadline_factory = timers.factory();
        let (deadlines, armer) = match timers.into_monitored() {
            Ok(parts) => parts,
            Err(_) => {
                return Err(Box::new(super::ActiveSetupFailure {
                    kind: super::ActiveSetupFailureKind::Failed,
                    observation,
                    deadline: None,
                    buffer: initial
                        .take()
                        .and_then(InitialWorkerResources::into_finite_buffer),
                }));
            }
        };
        let operation = match operation.activate(BridgeOperationKind::Subscription) {
            Ok(operation) => operation,
            Err(error) => {
                let _ = deadlines.finish_setup();
                let buffer = initial
                    .take()
                    .and_then(InitialWorkerResources::into_finite_buffer);
                return Err(Box::new(super::ActiveSetupFailure {
                    kind: if error == BridgeResourceError::Capacity {
                        super::ActiveSetupFailureKind::Capacity
                    } else {
                        super::ActiveSetupFailureKind::Failed
                    },
                    observation,
                    deadline: Some(InitialResponseDeadline::finite(deadlines, buffer)),
                    buffer: None,
                }));
            }
        };
        let (setup_sender, setup_receiver) = channel();
        let (initial_response, initial_task) = initial_response_channel();
        let (demand_sender, demand_receiver) = channel();
        let (event_sender, event_receiver) = channel();
        let fallbacks = initial
            .as_mut()
            .and_then(InitialWorkerResources::take_task_inputs);
        let initial_buffer = initial
            .as_mut()
            .and_then(InitialWorkerResources::take_response_buffer);
        let activity = validated.activity();
        let captured = validated
            .retained_telemetry()
            .and_then(chigiri_runtime::RequestTelemetry::propagation_context)
            .and_then(|values| {
                validated
                    .execution()
                    .schema
                    .instrumentation_with_values(values)
            });
        let instrumentation = captured.as_ref().map(|captured| {
            captured.retain_within(0).handoff_scope(
                chigiri_runtime::instrumentation::InstrumentationKind::SubscriptionSetup,
            )
        });
        let armer = armer
            .with_task(activity.clone())
            .with_instrumentation(captured.map(|captured| captured.retain_within(0)));
        let reporter = activity.is_managed().then(|| {
            crate::managed::HttpFaultReporter::subscription(validated.execution().schema.clone())
        });
        let work = super::task::run(
            validated,
            armer,
            setup_sender,
            initial_task,
            fallbacks,
            super::task::EventPorts {
                demand: demand_receiver,
                output: event_sender,
                activity: activity.clone(),
            },
        );
        let spawned = match reporter {
            Some(reporter) => operation.spawn_worker(
                super::super::managed_task::ManagedSubscriptionTask::new(
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
            Err(_) => {
                let _ = deadlines.finish_setup();
                if let Some(initial) = &mut initial {
                    initial.release_fallbacks();
                }
                return Err(Box::new(super::ActiveSetupFailure {
                    kind: super::ActiveSetupFailureKind::Failed,
                    observation,
                    deadline: Some(InitialResponseDeadline::finite(deadlines, initial_buffer)),
                    buffer: None,
                }));
            }
        };
        if activity.is_managed() {
            worker.retain_subscription_integrity(activity.clone());
        }
        Ok(ActiveWorkerSetup {
            worker: Some(ActiveSubscriptionWorker {
                activity,
                observation: None,
                operation: Some(operation),
                worker: Some(worker),
                deadlines: Some(deadlines),
                deadline_factory,
                demand_sender,
                event_receiver,
                initial_response,
                initial_resources: initial,
                pending_reservation: None,
                cleanup: None,
                terminal: None,
                ended: false,
                retained_telemetry: None,
            }),
            setup_receiver,
            initial_buffer,
            observation: Some(observation),
            transport,
        })
    }
}
