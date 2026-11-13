//! Execute the scoped authoring reference through real HTTP with every hook disabled.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::{BoxFuture, DeadlineFactory, Endpoint, FieldError, GraphQlRequest};
use chigiri_http::{ApplicationMetadata, BufferedBody, RequestHeader, TransportRequest};
use serde_json::json;

use super::{Client, HttpScope, Worker, endpoint};
use crate::ir::MockIr;
use crate::luau::{Contract, ScriptCandidate, ScriptSet, static_candidates};
use crate::reference::topic_runtime as examples;
use crate::store::Store;

struct Clock;

impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

fn candidates(ir: &MockIr) -> Vec<ScriptCandidate> {
    let mut candidates = static_candidates(ir).unwrap();
    for (coordinate, body) in [
        ("Query.user", examples::STORAGE_RESOLVER_EXAMPLE),
        (
            "Image.viewerLabel",
            examples::STORAGE_IMAGE_RESOLVER_EXAMPLE,
        ),
    ] {
        let (parent, name) = coordinate.split_once('.').unwrap();
        let parent = ir.graph().type_id(parent).unwrap();
        let field = ir.graph().types()[parent.0].fields[name];
        let index = ir
            .scripts()
            .iter()
            .position(|plan| plan.field == field)
            .unwrap();
        candidates[index].source =
            crate::luau::render_module(ir, candidates[index].script, body).unwrap();
    }
    candidates
}

#[tokio::test]
async fn ab_12_storage_reference_runs_private_parent_request_and_live_wallet_relation() {
    let _case = crate::vm_gate::test_case();
    let ir = Arc::new(
        MockIr::parse_with_storage(
            vec![chigiri::core::Source::new(
                "schema.graphql",
                examples::STORAGE_PUBLIC_EXAMPLE,
            )],
            vec![chigiri::core::Source::new(
                "storage.graphql",
                examples::STORAGE_SDL_EXAMPLE,
            )],
        )
        .unwrap(),
    );
    let seed = format!(
        r#"{{"User":[{}],"Wallet":[{}],"MockPreference":[{}]}}"#,
        examples::STORAGE_USER_SEED_EXAMPLE,
        examples::STORAGE_WALLET_SEED_EXAMPLE,
        examples::STORAGE_PREFERENCE_SEED_EXAMPLE,
    );
    let contract = Contract::new(ir.clone()).unwrap();
    let mut worker = Worker::start(
        contract,
        candidates(&ir),
        Store::from_seed_json(ir, &seed).unwrap(),
    )
    .unwrap();
    let client = worker.client.clone();
    let endpoint: Endpoint<Client> = endpoint(client, true, |_, _| {
        FieldError::public("Resolver failed").code("MOCK_RESOLVER_ERROR")
    })
    .unwrap();
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("x-chigiri-preflight", b"1"),
        RequestHeader::new("x-viewer", b"viewer-7"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let body = serde_json::to_vec(&json!({
        "query":"{user(id:\"u1\"){id avatar{url viewerLabel} wallet{id balance}}}"
    }))
    .unwrap();
    let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
    let response = scope
        .run(endpoint.handle(request, BufferedBody::new(body), &Clock))
        .await;
    assert_eq!(response.status, 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({"data":{"user":{"id":"u1","avatar":{"url":"/avatars/u1.png","viewerLabel":"viewer-7:u1"},"wallet":{"id":"w1","balance":125}}}})
    );
    assert!(scope.response_status().is_none());
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn authoring_guidance_request_and_date_examples_execute_without_casts() {
    let _case = crate::vm_gate::test_case();
    let ir = super::tests::ir(
        "type Query { headers:[String!] authorized:Boolean! later(deadline:String!):Boolean! }",
    );
    let contract = Contract::new(ir.clone()).unwrap();
    let edits = [
        ("Query.headers", examples::REQUEST_HEADERS_EXAMPLE),
        ("Query.authorized", examples::REQUEST_AUTH_RESOLVER_EXAMPLE),
        ("Query.later", examples::UTC_COMPARISON_EXAMPLE),
    ];
    let scripts = ScriptSet::new(contract, super::tests::candidates(&ir, &edits)).unwrap();
    let mut worker = Worker::from_checked_with_clock(
        scripts,
        vec![],
        Store::from_seed_json(ir, "{}").unwrap(),
        crate::clock::ClockSettings::fixed(0).unwrap(),
    )
    .unwrap();
    let client = worker.client.clone();
    let endpoint = super::tests::bound(client.clone());
    let absent = endpoint
        .execute(GraphQlRequest::new("{headers}"), client.clone(), &Clock)
        .await;
    assert!(absent.errors.is_empty());
    assert_eq!(
        chigiri_http::value_to_json(&absent.data.unwrap()).unwrap(),
        json!({"headers":null})
    );
    let denied = endpoint
        .execute(GraphQlRequest::new("{authorized}"), client.clone(), &Clock)
        .await;
    assert_eq!(denied.errors.len(), 1);
    assert_eq!(denied.errors[0].message, "An HTTP request is required.");

    for token in [None, Some("Bearer wrong"), Some("Bearer local-demo")] {
        let mut headers = vec![
            RequestHeader::new("content-type", b"application/json"),
            RequestHeader::new("x-chigiri-preflight", b"1"),
            RequestHeader::new("x-tag", b"first"),
            RequestHeader::new("x-tag", b"second"),
        ];
        if let Some(token) = token {
            headers.push(RequestHeader::new("Authorization", token.as_bytes()));
        }
        let metadata = ApplicationMetadata::default();
        let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let body = serde_json::to_vec(&json!({"query":
            "{headers authorized future:later(deadline:\"1970-01-01T00:00:01Z\") equal:later(deadline:\"1970-01-01T00:00:00Z\") past:later(deadline:\"1969-12-31T23:59:59Z\")}"
        })).unwrap();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap());
        let response = scope
            .run(endpoint.handle(request, BufferedBody::new(body), &Clock))
            .await;
        assert_eq!(response.status, 200);
        let value: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        if token == Some("Bearer local-demo") {
            assert!(value.get("errors").is_none(), "{value}");
            assert_eq!(value["data"]["authorized"], true);
            assert_eq!(value["data"]["future"], true);
            assert_eq!(value["data"]["equal"], false);
            assert_eq!(value["data"]["past"], false);
            assert_eq!(value["data"]["headers"], json!(["first", "second"]));
        } else {
            assert_eq!(value["errors"][0]["extensions"]["code"], "UNAUTHENTICATED");
        }
    }
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}
