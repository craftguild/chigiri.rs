//! Managed native tasks preserve finite request snapshots and correlation.

use super::*;
use axum::body::Body;
use tower::ServiceExt;

async fn native(fixture: &Fixture, scope: &HttpScope) -> (u16, serde_json::Value) {
    let (status, body) = native_with(fixture, scope, &[]).await;
    (status, serde_json::from_slice(&body).unwrap())
}

async fn native_with(
    fixture: &Fixture,
    scope: &HttpScope,
    headers: &[(&str, &str)],
) -> (u16, Vec<u8>) {
    let scopes = crate::service::HttpScopes::new().unwrap();
    scope
        .hold_admission(
            Arc::new(tokio::sync::Semaphore::new(crate::limits::HTTP_REQUESTS))
                .try_acquire_owned()
                .unwrap(),
        )
        .unwrap();
    let identity = scopes.bind(scope).unwrap();
    let endpoint = crate::service::managed_endpoint(
        fixture.worker.client.clone(),
        true,
        |_, _| FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR"),
        || HttpScope::new("127.0.0.1:49123".parse().unwrap()),
        scopes,
    )
    .unwrap();
    let service = chigiri_axum::service(endpoint.clone()).unwrap();
    let mut request = axum::http::Request::builder()
        .method("POST")
        .uri("/graphql")
        .header("host", "forged.example")
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut request = request
        .body(Body::from(r#"{"query":"{snapshot}"}"#))
        .unwrap();
    request.extensions_mut().insert(identity);
    let response = scope.run(service.clone().oneshot(request)).await.unwrap();
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    endpoint
        .shutdown_on(async {}, chigiri::ShutdownOptions::graceful())
        .await;
    drop(service);
    (status, body.to_vec())
}

#[tokio::test]
async fn managed_finite_execution_keeps_the_real_listener_request_snapshot() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{snapshot:String! @chigiri_mock_resolver}",
        "{}",
        &[(
            "Query.snapshot",
            r#"
            local raw = ctx.request
            if raw == ctx.null then return "none" end
            local request = raw :: ChigiriHookRequest
            return request.method .. "@" .. request.host .. ":" .. tostring(request.port) .. "@" .. request.url
        "#,
        )],
        &[],
    );
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    let (status, body) = native(&fixture, &scope).await;
    fixture.stop().await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        serde_json::json!({"data":{"snapshot":"POST@127.0.0.1:49123@/graphql"}})
    );
}

#[tokio::test]
async fn managed_finite_rejection_keeps_original_status_identity_and_hook_sink() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{snapshot:String! @chigiri_mock_resolver}",
        "{}",
        &[("Query.snapshot", "return 'unreachable'")],
        &[(
            HookSlot::RequestBefore,
            "ctx.log('info','managed') ctx.reject({code=403,message='denied'}) return nil",
        )],
    );
    let events = fixture.events.clone();
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
        .with_events(move |event| events.lock().unwrap().push(event));
    let id = scope.request_id();
    let (status, body) = native(&fixture, &scope).await;
    let observed_status = scope.response_status();
    let events = fixture
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|event| event.request_id)
        .collect::<Vec<_>>();
    fixture.stop().await;
    assert_eq!(status, 403);
    assert_eq!(body["errors"][0]["extensions"]["requestId"], id.to_string());
    assert_eq!(body["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
    assert_eq!(observed_status, Some(403));
    assert_eq!(events, [id]);
}

#[tokio::test]
async fn managed_upgrade_candidates_never_enter_finite_mock_hooks() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query{snapshot:String! @chigiri_mock_resolver}",
        "{}",
        &[("Query.snapshot", "return 'finite'")],
        &[(
            HookSlot::RequestBefore,
            "ctx.log('info','finite-entered') return nil",
        )],
    );
    let mut reports = Vec::new();
    for candidate in [
        None,
        Some(("connection", "keep-alive")),
        Some(("upgrade", "malformed")),
        Some(("sec-websocket-key", "malformed")),
        Some(("sec-websocket-version", "malformed")),
        Some(("sec-websocket-protocol", "malformed")),
        Some(("connection", "keep-alive, UpGrAdE")),
    ] {
        let events = fixture.events.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_events(move |event| events.lock().unwrap().push(event));
        let before = fixture.events.lock().unwrap().len();
        let (status, body) = native_with(&fixture, &scope, candidate.as_slice()).await;
        let hooks = fixture.events.lock().unwrap().len() - before;
        reports.push((candidate, status, body, hooks));
    }
    fixture.stop().await;
    for (candidate, status, body, hooks) in reports {
        let ordinary = candidate.is_none() || candidate == Some(("connection", "keep-alive"));
        assert_eq!(hooks, usize::from(ordinary), "candidate {candidate:?}");
        if ordinary {
            assert_eq!(status, 200);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                serde_json::json!({"data":{"snapshot":"finite"}})
            );
        } else {
            assert!(
                (400..500).contains(&status),
                "candidate {candidate:?}: {status}"
            );
        }
    }
}
