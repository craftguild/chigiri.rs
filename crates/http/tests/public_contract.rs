use chigiri_http::{RequestHeader, RequestParts, ResponseHeader, ResponseMediaType, ResponseParts};

#[test]
fn public_parts_can_be_built_without_a_server_library_type() {
    let headers = [RequestHeader::new("accept", b"application/json")];
    let request = RequestParts {
        method: "GET",
        uri_query: Some("query=%7B__typename%7D"),
        headers: &headers,
        body: b"",
    };
    assert_eq!(request.method, "GET");

    let response = ResponseParts {
        status: 200,
        headers: vec![ResponseHeader {
            name: "content-type".to_owned(),
            value: ResponseMediaType::Json.content_type().to_owned(),
        }],
        body: b"{}".to_vec(),
    };
    assert_eq!(response.status, 200);
}
