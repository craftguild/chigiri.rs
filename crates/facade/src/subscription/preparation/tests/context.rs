use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use chigiri_http::{RequestPhase, SubscriptionTransport};

use super::super::{ContextPreparation, prepare_context};
use super::fixture::{Counts, Factory, FactoryMode, HookState};
use super::support::{
    LifecycleRecords, NeverDeadline, SecondPollDeadline, block_on, observed_request, overall,
    poll_once, request,
};
use crate::transport::erase_factory;

#[test]
fn setup_reuses_preflight_telemetry_and_events_are_fresh() {
    let (schema, request) = request(SubscriptionTransport::ServerSentEvents);
    let counts = Arc::new(Counts::default());
    let factory = erase_factory::<(), HookState, _>(Factory {
        counts: Arc::clone(&counts),
        mode: FactoryMode::Ready,
    });
    let setup_phase = RequestPhase::SubscriptionSetup {
        transport: request.transport(),
    };
    let setup = prepare(&schema, &factory, &request, setup_phase, &NeverDeadline);
    let ContextPreparation::Ready(setup) = setup else {
        panic!("setup context must be ready");
    };
    assert_eq!(setup.execution_context(), &());
    assert_eq!(
        setup.telemetry(),
        request.request().request_telemetry().unwrap()
    );
    drop(setup);

    for sequence in [1, 2] {
        let event = prepare(
            &schema,
            &factory,
            &request,
            RequestPhase::SubscriptionEvent {
                transport: request.transport(),
                sequence,
            },
            &NeverDeadline,
        );
        let ContextPreparation::Ready(event) = event else {
            panic!("event context must be ready");
        };
        drop(event);
    }

    assert_eq!(counts.factory_calls.load(Ordering::SeqCst), 3);
    assert_eq!(counts.state_drops.load(Ordering::SeqCst), 3);
    assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 0);
    assert_eq!(counts.completion_drops.load(Ordering::SeqCst), 3);
    assert_eq!(
        *counts.phases.lock().unwrap(),
        vec![
            setup_phase,
            RequestPhase::SubscriptionEvent {
                transport: request.transport(),
                sequence: 1,
            },
            RequestPhase::SubscriptionEvent {
                transport: request.transport(),
                sequence: 2,
            },
        ]
    );
    assert_eq!(
        *counts.request_ids.lock().unwrap(),
        vec![
            Some("request-1".to_owned()),
            Some("request-2".to_owned()),
            Some("request-3".to_owned()),
        ]
    );
}

#[test]
fn rejection_and_internal_error_do_not_create_prepared_ownership() {
    let (schema, request) = request(SubscriptionTransport::Multipart);
    for mode in [FactoryMode::Reject, FactoryMode::Internal] {
        let counts = Arc::new(Counts::default());
        let factory = erase_factory::<(), HookState, _>(Factory {
            counts: Arc::clone(&counts),
            mode,
        });
        let result = prepare(
            &schema,
            &factory,
            &request,
            RequestPhase::SubscriptionSetup {
                transport: request.transport(),
            },
            &NeverDeadline,
        );
        match (mode, result) {
            (
                FactoryMode::Reject,
                ContextPreparation::Rejected {
                    rejection,
                    telemetry,
                },
            ) => {
                assert_eq!(rejection.status(), 403);
                assert_eq!(telemetry.request_id().unwrap().as_str(), "request-1");
            }
            (FactoryMode::Internal, ContextPreparation::Internal { error, telemetry }) => {
                assert_eq!(format!("{error:?}"), "HookError");
                assert_eq!(telemetry.request_id().unwrap().as_str(), "request-1");
            }
            _ => panic!("factory result classification changed"),
        }
        assert_eq!(counts.state_drops.load(Ordering::SeqCst), 0);
        assert_eq!(counts.completion_calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn context_timeout_after_poll_cancels_the_factory_future_without_ownership_escape() {
    let (schema, request) = request(SubscriptionTransport::ServerSentEvents);
    let counts = Arc::new(Counts::default());
    let factory = erase_factory::<(), HookState, _>(Factory {
        counts: Arc::clone(&counts),
        mode: FactoryMode::Pending,
    });
    let result = prepare(
        &schema,
        &factory,
        &request,
        RequestPhase::SubscriptionEvent {
            transport: request.transport(),
            sequence: 9,
        },
        &SecondPollDeadline,
    );
    let ContextPreparation::TimedOut { telemetry } = result else {
        panic!("immediate deadline must time out");
    };
    assert_eq!(telemetry.request_id().unwrap().as_str(), "request-2");
    assert_eq!(counts.factory_calls.load(Ordering::SeqCst), 1);
    assert_eq!(counts.future_drops.load(Ordering::SeqCst), 1);
    assert_eq!(counts.state_drops.load(Ordering::SeqCst), 0);
}

#[test]
fn context_construction_observes_each_terminal_classification() {
    use chigiri_runtime::{LifecyclePhase, LifecycleResult};

    for (mode, expected, code) in [
        (FactoryMode::Ready, LifecycleResult::Succeeded, None),
        (FactoryMode::Reject, LifecycleResult::Rejected, None),
        (
            FactoryMode::Internal,
            LifecycleResult::Failed,
            Some("INTERNAL_SERVER_ERROR"),
        ),
        (
            FactoryMode::Pending,
            LifecycleResult::TimedOut,
            Some("REQUEST_TIMEOUT"),
        ),
    ] {
        let records: LifecycleRecords = Arc::new(Mutex::new(Vec::new()));
        let (schema, request) =
            observed_request(SubscriptionTransport::Multipart, Arc::clone(&records));
        let counts = Arc::new(Counts::default());
        let factory = erase_factory::<(), HookState, _>(Factory { counts, mode });
        let deadline: &dyn chigiri_http::DeadlineFactory = if matches!(mode, FactoryMode::Pending) {
            &SecondPollDeadline
        } else {
            &NeverDeadline
        };
        let result = prepare(
            &schema,
            &factory,
            &request,
            RequestPhase::SubscriptionSetup {
                transport: request.transport(),
            },
            deadline,
        );
        drop(result);
        let context_events: Vec<_> = records
            .lock()
            .unwrap()
            .iter()
            .copied()
            .filter(|event| event.0 == LifecyclePhase::ContextConstruction)
            .collect();
        assert_eq!(
            context_events,
            vec![
                (
                    LifecyclePhase::ContextConstruction,
                    LifecycleResult::Started,
                    None
                ),
                (LifecyclePhase::ContextConstruction, expected, code),
            ]
        );
    }
}

#[test]
fn dropping_context_preparation_observes_cancellation() {
    use chigiri_runtime::{LifecyclePhase, LifecycleResult};

    let records: LifecycleRecords = Arc::new(Mutex::new(Vec::new()));
    let (schema, request) =
        observed_request(SubscriptionTransport::WebSocket, Arc::clone(&records));
    let counts = Arc::new(Counts::default());
    let factory = erase_factory::<(), HookState, _>(Factory {
        counts: Arc::clone(&counts),
        mode: FactoryMode::Pending,
    });
    let mut overall = overall();
    let mut preparation = Box::pin(prepare_context(
        &schema,
        &factory,
        &request,
        RequestPhase::SubscriptionSetup {
            transport: request.transport(),
        },
        &NeverDeadline,
        Duration::from_secs(1),
        overall.as_mut(),
        None,
    ));
    assert!(matches!(poll_once(preparation.as_mut()), Poll::Pending));
    drop(preparation);
    assert_eq!(counts.future_drops.load(Ordering::SeqCst), 1);
    let context_events: Vec<_> = records
        .lock()
        .unwrap()
        .iter()
        .copied()
        .filter(|event| event.0 == LifecyclePhase::ContextConstruction)
        .collect();
    assert_eq!(
        context_events,
        vec![
            (
                LifecyclePhase::ContextConstruction,
                LifecycleResult::Started,
                None
            ),
            (
                LifecyclePhase::ContextConstruction,
                LifecycleResult::Cancelled,
                None
            ),
        ]
    );
}

fn prepare(
    schema: &chigiri_runtime::ExecutableSchema<()>,
    factory: &Arc<dyn crate::transport::ErasedContextFactory<()>>,
    request: &crate::subscription::request::SubscriberRequest,
    phase: RequestPhase,
    deadlines: &dyn chigiri_http::DeadlineFactory,
) -> ContextPreparation<()> {
    let mut overall = overall();
    block_on(prepare_context(
        schema,
        factory,
        request,
        phase,
        deadlines,
        Duration::from_secs(1),
        overall.as_mut(),
        None,
    ))
}
