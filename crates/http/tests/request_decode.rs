use chigiri_core::Value;
use chigiri_http::{RequestHeader, RequestMethod, RequestParts, ResponseMediaType, decode_request};

fn post<'a>(body: &'a [u8], headers: &'a [RequestHeader<'a>]) -> RequestParts<'a> {
    RequestParts {
        method: "POST",
        uri_query: None,
        headers,
        body,
    }
}

#[test]
fn decodes_query_operation_variables_and_extensions_from_post_json() {
    let headers = [
        RequestHeader::new("content-type", b"application/json; charset=UTF-8"),
        RequestHeader::new(
            "accept",
            b"application/json;q=0.5, application/graphql-response+json",
        ),
    ];
    let decoded = decode_request(post(
        br#"{"query":"query Q($id: ID!){book(id:$id){title}}","operationName":"Q","variables":{"id":7},"extensions":{"trace":true},"future":"ignored"}"#,
        &headers,
    ))
    .unwrap();

    assert_eq!(decoded.method, RequestMethod::Post);
    assert_eq!(
        decoded.response_media_type,
        ResponseMediaType::GraphQlResponseJson
    );
    assert_eq!(decoded.request.operation_name.as_deref(), Some("Q"));
    assert_eq!(decoded.request.variables.get("id"), Some(&Value::Int(7)));
    assert_eq!(
        decoded.request.extensions.get("trace"),
        Some(&Value::Boolean(true))
    );
}

#[test]
fn decodes_json_numbers_with_empty_fractions_as_lossless_integers() {
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let decoded = decode_request(post(
        br#"{"query":"query Q($int:Int,$id:ID,$float:Float){x}","variables":{"int":1.0,"id":1.0,"float":9007199254740993.0}}"#,
        &headers,
    ))
    .unwrap();

    assert_eq!(decoded.request.variables.get("int"), Some(&Value::Int(1)));
    assert_eq!(decoded.request.variables.get("id"), Some(&Value::Int(1)));
    assert_eq!(
        decoded.request.variables.get("float"),
        Some(&Value::Int(9_007_199_254_740_993))
    );

    let decoded = decode_request(post(
        br#"{"query":"query Q($id:ID){x}","variables":{"id":1606938044258990275541962092341162602522202993782792835301376}}"#,
        &headers,
    ))
    .unwrap();
    assert_eq!(
        decoded.request.variables.get("id"),
        Some(&Value::BigInt(
            "1606938044258990275541962092341162602522202993782792835301376".to_owned()
        ))
    );
}

#[test]
fn decodes_get_parameters_and_empty_optional_values() {
    let headers = [RequestHeader::new("accept", b"application/json")];
    let decoded = decode_request(RequestParts {
        method: "GET",
        uri_query: Some(
            "query=query+Q%28%24id%3AID%21%29%7Bbook%28id%3A%24id%29%7Btitle%7D%7D&operationName=&variables=%7B%22id%22%3A%22b1%22%7D&extensions=",
        ),
        headers: &headers,
        body: b"ignored",
    })
    .unwrap();

    assert_eq!(decoded.method, RequestMethod::Get);
    assert_eq!(decoded.response_media_type, ResponseMediaType::Json);
    assert_eq!(decoded.request.operation_name, None);
    assert_eq!(
        decoded.request.variables.get("id").and_then(Value::as_str),
        Some("b1")
    );
    assert!(decoded.request.extensions.is_empty());
}

#[test]
fn maps_content_type_accept_and_method_failures_to_status_codes() {
    let accept = [RequestHeader::new(
        "accept",
        b"application/graphql-response+json",
    )];
    assert_eq!(
        decode_request(post(b"{}", &accept)).unwrap_err().status(),
        415
    );

    let headers = [
        RequestHeader::new("content-type", b"text/plain"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    assert_eq!(
        decode_request(post(b"{}", &headers)).unwrap_err().status(),
        415
    );

    let headers = [RequestHeader::new("accept", b"text/html")];
    assert_eq!(
        decode_request(post(b"{}", &headers)).unwrap_err().status(),
        406
    );

    let error = decode_request(RequestParts {
        method: "PUT",
        uri_query: None,
        headers: &accept,
        body: b"",
    })
    .unwrap_err();
    let response = error.into_response_parts();
    assert_eq!(response.status, 405);
    assert!(response.headers.iter().any(|header| header.name == "allow"));

    let error = decode_request(RequestParts {
        method: "get",
        uri_query: Some("query=%7Bhello%7D"),
        headers: &accept,
        body: b"",
    })
    .unwrap_err();
    assert_eq!(error.status(), 405);
}

#[test]
fn json_syntax_failures_return_400() {
    let headers = [RequestHeader::new("content-type", b"application/json")];
    assert_eq!(
        decode_request(post(b"{", &headers)).unwrap_err().status(),
        400
    );

    for body in [
        br#"[]"#.as_slice(),
        br#"{"query":null}"#,
        br#"{"query":"{x}","variables":[]}"#,
    ] {
        assert_eq!(
            decode_request(post(body, &headers)).unwrap_err().status(),
            422
        );
    }

    for uri_query in [None, Some("operationName=Q")] {
        let error = decode_request(RequestParts {
            method: "GET",
            uri_query,
            headers: &[],
            body: b"",
        })
        .unwrap_err();
        assert_eq!(error.status(), 422);
    }

    let malformed_uri = decode_request(RequestParts {
        method: "GET",
        uri_query: Some("query=%GG"),
        headers: &[],
        body: b"",
    })
    .unwrap_err();
    assert_eq!(malformed_uri.status(), 400);
}

#[test]
fn post_omits_optional_nulls_and_preserves_empty_query_strings() {
    let headers = [RequestHeader::new("content-type", b"application/json")];
    let decoded = decode_request(post(
        br#"{"query":"","operationName":null,"variables":null,"extensions":null}"#,
        &headers,
    ))
    .unwrap();
    assert!(decoded.request.query.is_empty());
    assert_eq!(decoded.request.operation_name, None);
    assert!(decoded.request.variables.is_empty());
    assert!(decoded.request.extensions.is_empty());
}

#[test]
fn invalid_persisted_envelopes_return_400_for_exclusive_media() {
    let headers = [
        RequestHeader::new("content-type", b"application/json"),
        RequestHeader::new("accept", b"application/graphql-response+json"),
    ];
    for body in [
        br#"{"extensions":{"persistedQuery":{"version":2,"sha256Hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}}"#.as_slice(),
        br#"{"extensions":{"persistedQuery":{"version":1,"sha256Hash":7}}}"#,
        br#"{"extensions":{"persistedQuery":{"version":1,"sha256Hash":"not-a-digest"}}}"#,
    ] {
        let response = decode_request(post(body, &headers))
            .unwrap_err()
            .into_response_parts();
        assert_eq!(response.status, 400);
        assert_eq!(
            response.headers[0].value,
            "application/graphql-response+json; charset=utf-8"
        );
        assert!(std::str::from_utf8(&response.body).unwrap().contains("BAD_REQUEST"));
    }
}
