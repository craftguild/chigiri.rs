use chigiri_core::{GraphQlError, GraphQlResponse, Location, Object, Value};
use chigiri_http::{
    ResolvedOperationSource, ResponseHeader, ResponseMediaType, ResponseParts, encode_response,
    encode_response_with_status,
};

#[test]
fn json_encoding_preserves_response_fields_and_data_object_order() {
    let mut data = Object::new();
    data.insert("second", 2_i32);
    data.insert("first", 1_i32);
    let response = GraphQlResponse::executed(Value::Object(data)).with_error({
        let mut error = GraphQlError::new("bad field").code("BAD_FIELD");
        error.locations.push(Location::new(2, 3));
        error.path.push("second".into());
        error
    });

    let encoded = encode_response(ResponseMediaType::GraphQlResponseJson, &response).unwrap();
    assert_eq!(encoded.status, 200);
    assert_eq!(
        String::from_utf8(encoded.body).unwrap(),
        r#"{"data":{"second":2,"first":1},"errors":[{"message":"bad field","locations":[{"line":2,"column":3}],"path":["second"],"extensions":{"code":"BAD_FIELD"}}]}"#
    );
}

#[test]
fn request_errors_use_400_or_422_for_graphql_media_and_200_for_legacy_json() {
    let response = GraphQlResponse::request_error(
        GraphQlError::new("bad document").code("GRAPHQL_PARSE_FAILED"),
    );
    assert_eq!(
        encode_response(ResponseMediaType::GraphQlResponseJson, &response)
            .unwrap()
            .status,
        400
    );
    assert_eq!(
        encode_response(ResponseMediaType::Json, &response)
            .unwrap()
            .status,
        200
    );

    for code in [
        "GRAPHQL_VALIDATION_FAILED",
        "OPERATION_NOT_FOUND",
        "BAD_USER_INPUT",
    ] {
        let response = GraphQlResponse::request_error(GraphQlError::new("bad request").code(code));
        assert_eq!(
            encode_response(ResponseMediaType::GraphQlResponseJson, &response)
                .unwrap()
                .status,
            422
        );
    }
}

#[test]
fn get_mutation_is_405_and_allow_header() {
    let response = GraphQlResponse::request_error(
        GraphQlError::new("GET requests may only execute query operations")
            .code("METHOD_NOT_ALLOWED"),
    );
    let encoded = encode_response(ResponseMediaType::GraphQlResponseJson, &response).unwrap();
    assert_eq!(encoded.status, 405);
    assert!(
        encoded
            .headers
            .iter()
            .any(|header| header.name == "allow" && header.value == "GET, POST")
    );
}

#[test]
fn adapter_failures_preserve_graphql_response_shape_with_the_configured_status() {
    let response = GraphQlResponse::request_error(
        GraphQlError::new("Request timed out").code("REQUEST_TIMEOUT"),
    );
    let encoded =
        encode_response_with_status(ResponseMediaType::GraphQlResponseJson, 504, &response)
            .unwrap();
    assert_eq!(encoded.status, 504);
    let json: serde_json::Value = serde_json::from_slice(&encoded.body).unwrap();
    assert_eq!(json["errors"][0]["extensions"]["code"], "REQUEST_TIMEOUT");
}

#[test]
fn non_json_float_produces_a_serialization_error() {
    let response = GraphQlResponse::executed(Value::Float(f64::NAN));
    assert!(encode_response(ResponseMediaType::GraphQlResponseJson, &response).is_err());

    for invalid in ["1", "1.5", "1e2", "01"] {
        let response = GraphQlResponse::executed(Value::BigInt(invalid.to_owned()));
        assert!(
            encode_response(ResponseMediaType::GraphQlResponseJson, &response).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn response_debug_omits_headers_and_body() {
    let response = ResponseParts {
        status: 200,
        headers: vec![ResponseHeader {
            name: "set-cookie".to_owned(),
            value: "session=debug-secret".to_owned(),
        }],
        body: b"private-response-body".to_vec(),
    };
    let debug = format!("{response:?}");

    assert!(!debug.contains("debug-secret"));
    assert!(!debug.contains("private-response-body"));
    assert!(debug.contains("value_bytes"));
    assert!(debug.contains("body_bytes"));
}

#[test]
fn resolved_source_debug_redacts_raw_documents() {
    let source = ResolvedOperationSource::new("query { privatePassword(value: 1234) }")
        .with_catalog_cost_hint(7);
    let debug = format!("{source:?}");

    assert!(!debug.contains("privatePassword"));
    assert!(!debug.contains("1234"));
    assert!(debug.contains("document_bytes"));
    assert!(debug.contains("has_catalog_cost_hint"));
}
