//! Strict differential-corpus reader and Axum execution helper.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Method, Request};
use chigiri_axum_adoption::differential::ObservedResponse;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;

pub(crate) struct CorpusCase {
    pub(crate) id: String,
    pub(crate) query: String,
    pub(crate) operation_name: Option<String>,
    pub(crate) variables: Value,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) compare_headers: Vec<String>,
    pub(crate) volatile_extension_pointers: Vec<String>,
    pub(crate) legacy: ObservedResponse,
}

pub(crate) fn load_cases(source: &str) -> Vec<CorpusCase> {
    let value: Value = serde_json::from_str(source).unwrap();
    let cases = value
        .as_array()
        .expect("corpus root must be an array")
        .iter()
        .map(parse_case)
        .collect::<Vec<_>>();
    let mut ids = std::collections::BTreeSet::new();
    for case in &cases {
        assert!(
            !case.id.trim().is_empty(),
            "corpus case ID must not be empty"
        );
        assert!(ids.insert(&case.id), "The corpus case ID is duplicated.");
    }
    cases
}

fn parse_case(value: &Value) -> CorpusCase {
    let object = value.as_object().expect("The corpus case is the object.");
    let keys = object
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let required = ["id", "query", "headers", "compareHeaders", "legacy"]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let allowed = required
        .iter()
        .copied()
        .chain(["operationName", "variables", "volatileExtensionPointers"])
        .collect::<std::collections::BTreeSet<_>>();
    assert!(required.is_subset(&keys));
    assert!(keys.is_subset(&allowed));
    CorpusCase {
        id: string(object, "id"),
        query: string(object, "query"),
        operation_name: object
            .get("operationName")
            .map(|value| value.as_str().unwrap().to_owned()),
        variables: object
            .get("variables")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
        headers: string_map(&object["headers"]),
        compare_headers: object["compareHeaders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect(),
        volatile_extension_pointers: object
            .get("volatileExtensionPointers")
            .map(string_list)
            .unwrap_or_default(),
        legacy: parse_response(&object["legacy"]),
    }
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

fn parse_response(value: &Value) -> ObservedResponse {
    let object = value.as_object().unwrap();
    ObservedResponse {
        status: object["status"].as_u64().unwrap() as u16,
        media_type: object["mediaType"].as_str().unwrap().to_owned(),
        headers: string_list_map(&object["headers"]),
        body: object["body"].clone(),
    }
}

fn string(object: &serde_json::Map<String, Value>, key: &str) -> String {
    object[key].as_str().unwrap().to_owned()
}

fn string_map(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_owned()))
        .collect()
}

fn string_list_map(value: &Value) -> BTreeMap<String, Vec<String>> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

pub(crate) async fn execute_new(cases: &[CorpusCase]) -> Vec<ObservedResponse> {
    let router = chigiri_axum_adoption::router();
    let mut responses = Vec::with_capacity(cases.len());
    for case in cases {
        let body = serde_json::to_vec(&serde_json::json!({
            "query": case.query,
            "operationName": case.operation_name,
            "variables": case.variables,
        }))
        .unwrap();
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("accept", "application/graphql-response+json")
            .body(Body::from(body))
            .unwrap();
        for (name, value) in &case.headers {
            request.headers_mut().insert(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let media_type = response.headers()["content-type"]
            .to_str()
            .unwrap()
            .to_owned();
        let mut headers = BTreeMap::new();
        for name in &case.compare_headers {
            let values = response
                .headers()
                .get_all(name)
                .iter()
                .map(|value| value.to_str().unwrap().to_owned())
                .collect();
            headers.insert(name.clone(), values);
        }
        let body = response.into_body().collect().await.unwrap().to_bytes();
        responses.push(ObservedResponse {
            status,
            media_type,
            headers,
            body: serde_json::from_slice(&body).unwrap(),
        });
    }
    responses
}
