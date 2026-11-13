//! Connects the generated custom scalar to a transport-neutral HTTP endpoint and bounded writer.

use std::time::Duration;

use chigiri::{Endpoint, ExecutionLimits, GraphQlRequest, PathSegment};
use chigiri_http::{
    ApplicationMetadata, BoxFuture, BufferedBody, DeadlineFactory, RequestHeader,
    ResponseMediaType, ResponseParts, TransportRequest, encode_response,
};

use crate::direct::EXACT_QUERY;
use crate::fixture::{
    Recorder, Schema, block_on, gated_schema, invalid_builtin_schema, observed_schema, schema,
    schema_with_response_limit,
};

#[test]
fn preserves_exact_object_list_and_custom_json_order() {
    let endpoint = endpoint(schema(ExecutionLimits::production()), None);
    let response = post(endpoint, EXACT_QUERY);

    assert_eq!(response.status, 200);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        concat!(
            "{\"data\":{\"nullValue\":null,\"booleanValue\":true,",
            "\"intValue\":42,\"bigIntValue\":170141183460469231731687303715884105728,",
            "\"floatValue\":1.5,\"stringValue\":\"text\",",
            "\"objectList\":{\"items\":[1,null]},",
            "\"listObject\":[{\"ok\":true}],",
            "\"ordered\":{\"zeta\":[\"first\",2],\"alpha\":null}}}"
        )
    );
}

#[test]
fn cso_12_http_preserves_ids_sources_paths_coordinates_and_masking() {
    let (executable, recorder) = observed_schema(ExecutionLimits::production());
    let response = post(endpoint(executable, None), "{ json(mode: \"bad-bigint\") }");

    assert_eq!(response.status, 200);
    let body = String::from_utf8(response.body.clone()).unwrap();
    assert!(body.contains("\"message\":\"Internal server error\""));
    assert!(body.contains("\"errorId\":\"cso-1\""));
    assert!(body.contains("\"path\":[\"json\"]"));
    assert!(!body.contains("01-private"));
    assert!(!body.contains("custom scalar output"));
    assert!(!format!("{response:?}").contains("01-private"));
    assert_http_record(
        &recorder,
        "The custom-scalar output has an invalid numeric representation.",
    );

    let mut limits = ExecutionLimits::production();
    limits.max_custom_scalar_output_depth = 1;
    let (executable, recorder) = observed_schema(limits);
    let response = post(endpoint(executable, None), "{ json(mode: \"depth-2\") }");
    assert!(String::from_utf8_lossy(&response.body).contains("\"errorId\":\"cso-1\""));
    assert!(!format!("{response:?}").contains("response output limit exceeded"));
    assert_masked_http(response);
    assert_http_record(&recorder, "response output limit exceeded");
}

#[test]
fn cso_03_http_also_rejects_the_container_and_null_of_built_in_scalar() {
    let response = post(
        endpoint(invalid_builtin_schema(ExecutionLimits::production()), None),
        concat!(
            "{ wrongKind: text nullValue: text ",
            "objectValue: text listValue: text }"
        ),
    );

    assert_eq!(response.status, 200);
    let body = String::from_utf8(response.body).unwrap();
    assert_eq!(body.matches("Internal server error").count(), 4);
    for key in ["wrongKind", "nullValue", "objectValue", "listValue"] {
        assert!(body.contains(&format!("\"{key}\":null")));
    }
}

#[test]
fn cso_13_reports_one_encoded_byte_and_masks_failures_with_500() {
    let query = "{ json(mode: \"large\") }";
    let direct =
        block_on(schema(ExecutionLimits::production()).execute(GraphQlRequest::new(query), ()));
    let encoded = encode_response(ResponseMediaType::GraphQlResponseJson, &direct).unwrap();
    assert!(encoded.body.len() > 128);

    let exact = post(
        endpoint(
            schema(ExecutionLimits::production()),
            Some(encoded.body.len()),
        ),
        query,
    );
    assert_eq!(exact.status, 200);
    assert_eq!(exact.body, encoded.body);

    let below = post(
        endpoint(
            schema(ExecutionLimits::production()),
            Some(encoded.body.len() - 1),
        ),
        query,
    );
    assert_eq!(below.status, 500);
    let body = String::from_utf8(below.body).unwrap();
    assert!(body.contains("Internal server error"));
    assert!(!body.contains(&"x".repeat(32)));
}

#[test]
fn cso_05_06_08_09_11_resource_boundary_and_bubbling_also_mask_the_http_path() {
    let mut depth = ExecutionLimits::production();
    depth.max_custom_scalar_output_depth = 1;
    assert_masked_http(post(
        endpoint(schema(depth), None),
        "{ json(mode: \"depth-2\") }",
    ));

    let mut deep = ExecutionLimits::production();
    deep.max_custom_scalar_output_depth = 10_000;
    let deep_response = post(
        endpoint(schema(deep), None),
        "{ json(mode: \"deep-invalid\") }",
    );
    let debug = format!("{deep_response:?}");
    assert!(!debug.contains("01-deep-private"));
    assert!(!debug.contains("10000"));
    assert_masked_http(deep_response);

    let mut nodes = ExecutionLimits::production();
    nodes.max_output_nodes = 3;
    assert_masked_http(post(
        endpoint(schema(nodes), None),
        "{ json(mode: \"nested-nodes\") }",
    ));

    let mut items = ExecutionLimits::production();
    items.max_list_items_per_field = 2;
    assert_masked_http(post(
        endpoint(schema(items), None),
        "{ json(mode: \"nested-list-3\") }",
    ));

    let bubbled = post(
        endpoint(schema(ExecutionLimits::production()), None),
        "{ requiredJson(mode: \"nan\") }",
    );
    assert_eq!(bubbled.status, 200);
    assert!(
        String::from_utf8(bubbled.body)
            .unwrap()
            .starts_with("{\"data\":null,")
    );
}

#[test]
fn cso_10_http_concurrent_sibling_also_shares_global_budget() {
    let mut limits = ExecutionLimits::production();
    limits.max_output_nodes = 3;
    let response = post(
        endpoint(gated_schema(limits), None),
        concat!(
            "{ left: json(mode: \"gated-left\") ",
            "right: json(mode: \"gated-right\") }"
        ),
    );
    let body = String::from_utf8(response.body).unwrap();
    assert_eq!(body.matches("Internal server error").count(), 1);
    assert_eq!(body.matches(":null").count(), 1);
}

#[test]
fn preserves_input_containers_and_serializes_field_errors() {
    let input = post(
        endpoint(schema(ExecutionLimits::production()), None),
        "{ echoJson(value: {object: [1, null, {ok: true}]}) }",
    );
    assert_eq!(input.status, 200);
    assert_eq!(
        String::from_utf8(input.body).unwrap(),
        "{\"data\":{\"echoJson\":{\"object\":[1,null,{\"ok\":true}]}}}"
    );

    let (executable, recorder) = observed_schema(ExecutionLimits::production());
    let failure = post(
        endpoint(executable, None),
        "{ json(mode: \"serialize-error\") }",
    );
    assert_masked_http(failure);
    let records = recorder.records();
    assert_eq!(records[0].source, "synthetic scalar serialize failure");
    assert_eq!(records[0].component, "resolver");
}

fn endpoint(
    executable: Schema,
    response_limit: Option<usize>,
) -> Endpoint<()> {
    match response_limit {
        Some(limit) => schema_with_response_limit(limit).endpoint,
        None => executable.endpoint,
    }
}

fn post(endpoint: Endpoint<()>, query: &str) -> ResponseParts {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    let metadata = ApplicationMetadata::default();
    let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
    let escaped = query.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!("{{\"query\":\"{escaped}\"}}").into_bytes();
    block_on(endpoint.handle(request, BufferedBody::new(body), &NeverDeadline))
}

fn assert_http_record(recorder: &Recorder, source: &str) {
    let records = recorder.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].error_id.as_deref(), Some("cso-1"));
    assert_eq!(records[0].source, source);
    assert_eq!(records[0].path, [PathSegment::from("json")]);
    assert_eq!(records[0].coordinate.as_deref(), Some("Query.json"));
    assert_eq!(records[0].component, "completion");
}

fn assert_masked_http(response: ResponseParts) {
    assert_eq!(response.status, 200);
    let body = String::from_utf8(response.body).unwrap();
    assert!(body.contains("\"message\":\"Internal server error\""));
    assert!(!body.contains("01-deep-private"));
    assert!(!body.contains("response output limit exceeded"));
}

struct NeverDeadline;

impl DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: Duration) -> BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}
