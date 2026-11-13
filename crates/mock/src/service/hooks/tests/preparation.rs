//! Prove phase ownership without retaining setup state, budgets, or apparent time.

use super::*;
use chigiri::{GraphQlRequest, HttpRequestContext, RequestMethod, RequestPhase};
use chigiri_http::{ResponseMediaType, SubscriptionTransport};

fn prepare(client: Client, phase: RequestPhase) -> (Client, bool) {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("x-subscriber", b"private-subscriber")];
    let transport = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let request = GraphQlRequest::new("subscription { event }");
    let context = HttpRequestContext::__new(
        transport,
        &request,
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
    )
    .__with_raw_body(b"must-not-survive-streaming-preparation")
    .__with_phase(phase, None);
    let prepared = super::super::prepared_scoped(
        client,
        context,
        HttpScope::new("127.0.0.1:49123".parse().unwrap()),
    );
    let (client, (), completion) = prepared.__into_parts();
    (client, completion.is_some())
}

fn event(sequence: u64) -> RequestPhase {
    RequestPhase::SubscriptionEvent {
        transport: SubscriptionTransport::ServerSentEvents,
        sequence,
    }
}

#[tokio::test]
async fn streaming_preparation_uses_fresh_state_budget_clock_and_cancellation() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new_with_clock(
        "type Query { ping: Int }",
        "{}",
        &[("Query.ping", "return 1")],
        &[],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    let ambient = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    ambient.0.cancel.cancel();
    let (first, _) = ambient
        .run(async { prepare(fixture.worker.client.clone(), event(1)) })
        .await;
    let first_session = first.session.as_ref().unwrap();
    assert!(first_session.active(&first));
    first_session
        .state
        .lock()
        .unwrap()
        .apply_patch(
            HookSlot::RequestBefore,
            &chigiri::Value::Object(chigiri::Object::from_iter([("count", 99_i32)])),
            &mut ValueBudget::default(),
        )
        .unwrap();
    first_session
        .trace
        .execution_budget
        .clock()
        .travel_to(999)
        .unwrap();
    first_session.trace.execution_budget.0.clone().fail();
    fixture
        .worker
        .control()
        .run_luau("timeleap.travel_to(200, {world=true})")
        .unwrap();

    let (second, completion) = prepare(fixture.worker.client.clone(), event(2));
    let second_session = second.session.as_ref().unwrap();
    assert!(!completion);
    assert_ne!(first_session.trace.id, second_session.trace.id);
    assert!(!Arc::ptr_eq(&first_session.trace, &second_session.trace));
    assert!(second_session.active(&second));
    assert!(second_session.trace.execution_budget.failure().is_none());
    assert_eq!(
        second_session.trace.execution_budget.clock().now().unwrap(),
        200
    );
    assert_eq!(
        first_session.trace.execution_budget.clock().now().unwrap(),
        999
    );
    assert_eq!(
        second_session.state.lock().unwrap().snapshot().get("count"),
        Some(&chigiri::Value::Int(1))
    );
    let request = second_session.execution(&second).unwrap().request.unwrap();
    let chigiri::Value::Object(request) = request.as_ref() else {
        panic!("request metadata was not an object")
    };
    assert_eq!(request.get("body"), Some(&chigiri::Value::from("")));
    let chigiri::Value::List(headers) = request.get("headers").unwrap() else {
        panic!("missing subscriber headers")
    };
    assert_eq!(headers.len(), 1);
    drop(first);
    assert!(second_session.active(&second));
    drop(second);
    fixture.stop().await;
}

#[tokio::test]
async fn only_initial_http_setup_owns_response_completion() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query { ping: Int }",
        "{}",
        &[("Query.ping", "return 1")],
        &[],
    );
    for (phase, completion) in [
        (
            RequestPhase::SubscriptionSetup {
                transport: SubscriptionTransport::ServerSentEvents,
            },
            true,
        ),
        (
            RequestPhase::SubscriptionSetup {
                transport: SubscriptionTransport::Multipart,
            },
            true,
        ),
        (
            RequestPhase::SubscriptionSetup {
                transport: SubscriptionTransport::WebSocket,
            },
            false,
        ),
        (event(1), false),
        (
            RequestPhase::WebSocketOperation {
                operation: chigiri_http::WebSocketOneShot::Query,
            },
            false,
        ),
    ] {
        let (client, actual) = prepare(fixture.worker.client.clone(), phase);
        assert_eq!(actual, completion, "{phase:?}");
        assert!(client.session.as_ref().unwrap().request);
    }
    fixture.stop().await;
}

#[tokio::test]
async fn fatal_query_classifies_every_typed_host_failure_without_response_inspection() {
    use super::super::completion::Failure;
    use crate::limits::request::{LimitKind, LimitNotice, LocalLimit, policy};

    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query { ping: Int }",
        "{}",
        &[("Query.ping", "return 1")],
        &[],
    );
    let (client, _) = prepare(fixture.worker.client.clone(), event(1));
    let session = client.session.as_ref().unwrap();
    assert!(!session.event_execution_failed());
    for failure in [
        Failure::Internal,
        Failure::SourceStopped,
        Failure::Timeout,
        Failure::Limit(LimitNotice {
            kind: LimitKind::Work,
            observed: policy(LimitKind::Work).hard + 1,
        }),
        Failure::LocalLimit(LocalLimit::PcallReentry),
        Failure::OutputLimit,
    ] {
        *session.failure.lock().unwrap() = Some(failure);
        assert!(session.event_execution_failed());
    }
    *session.failure.lock().unwrap() = Some(Failure::Rejected(
        403,
        chigiri::GraphQlError::new("opaque decision"),
    ));
    assert!(!session.event_execution_failed());
    *session.failure.lock().unwrap() = None;
    drop(client);
    fixture.stop().await;
}

#[tokio::test]
async fn stopped_source_cancels_in_flight_work_and_preserves_the_source_error_mask() {
    use super::super::completion::Failure;

    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query { ping: Int }",
        "{}",
        &[("Query.ping", "return 1")],
        &[],
    );
    let (normal, _) = prepare(fixture.worker.client.clone(), event(1));
    let session = normal.session.as_ref().unwrap();
    session.fail(Failure::Internal);
    let failure = session.failure.lock().unwrap().clone().unwrap();
    assert_eq!(
        session.failure_response(&failure).errors[0].message,
        "Mock execution failed"
    );
    drop(normal);
    let (client, _) = prepare(fixture.worker.client.clone(), event(2));
    let session = client.session.as_ref().unwrap();
    let source = crate::cancellation::Cancellation::default();
    session.source_cancel.set(source.clone()).ok().unwrap();
    let execution = session.execution(&client).unwrap();
    assert!(!execution.cancel.cancelled());
    assert!(session.active(&client));
    source.cancel();
    assert!(execution.cancel.cancelled());
    assert!(!session.active(&client));
    session.observe_execution_failure();
    assert!(session.event_execution_failed());
    let failure = session.failure.lock().unwrap().clone().unwrap();
    let response = session.failure_response(&failure);
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&chigiri::Value::from("INTERNAL_SERVER_ERROR"))
    );
    drop(client);
    fixture.stop().await;
}
