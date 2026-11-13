use serde_json::Value;

use super::client::HttpResponse;

#[path = "validation/metadata.rs"]
mod metadata;

pub fn assert_sse_ticks(response: &HttpResponse) {
    assert_stream_head(response, "text/event-stream; charset=utf-8");
    let entries = sse_entries(&response.body);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].0, "next");
    assert_eq!(entries[1].0, "next");
    assert_eq!(entries[2], ("complete", ""));
    for (entry, expected) in entries[..2].iter().zip([1, 2]) {
        let payload = parse(entry.1);
        assert_eq!(payload["data"]["ticks"]["value"], expected);
        assert_default_delivery(&payload);
    }
}

pub fn assert_multipart_ticks(response: &HttpResponse) {
    assert_stream_head(
        response,
        "multipart/mixed; boundary=graphql; subscriptionSpec=1.0",
    );
    let entries = multipart_entries(&response.body);
    assert_eq!(entries.len(), 2);
    for (entry, expected) in entries.iter().zip([1, 2]) {
        assert_eq!(entry["payload"]["data"]["ticks"]["value"], expected);
        assert_default_delivery(&entry["payload"]);
    }
    assert!(response.body.ends_with(b"--graphql--"));
}

pub fn assert_selected_error(accept: &str, response: &HttpResponse, expected_code: &str) {
    assert_eq!(
        response.status, 200,
        "selected {accept} error {expected_code} must stay on its streaming profile"
    );
    if accept == "text/event-stream" {
        assert_stream_head(response, "text/event-stream; charset=utf-8");
        let entries = sse_entries(&response.body);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "next");
        assert_eq!(entries[1], ("complete", ""));
        let payload = parse(entries[0].1);
        assert_eq!(payload["errors"][0]["extensions"]["code"], expected_code);
    } else {
        assert_stream_head(
            response,
            "multipart/mixed; boundary=graphql; subscriptionSpec=1.0",
        );
        let entries = multipart_entries(&response.body);
        assert_eq!(entries.len(), 1);
        assert!(entries[0]["payload"].is_null());
        let error = &entries[0]["errors"][0];
        assert_eq!(error["extensions"]["code"], expected_code);
        assert!(error.get("path").is_none());
        assert!(error.get("locations").is_none());
        let text = std::str::from_utf8(&response.body).expect("multipart response UTF-8");
        let error_position = text.find(expected_code).expect("multipart error entry");
        let close_position = text.rfind("--graphql--").expect("multipart close entry");
        assert!(error_position < close_position);
    }
}

pub fn assert_sse_error(response: &HttpResponse) {
    assert_stream_head(response, "text/event-stream; charset=utf-8");
    let entries = sse_entries(&response.body);
    assert_eq!(entries.len(), 3);
    let event = parse(entries[0].1);
    assert_eq!(event["data"]["failingTicks"]["value"], 7);
    assert_default_delivery(&event);
    assert_eq!(entries[1].0, "next");
    assert_eq!(entries[2], ("complete", ""));
    let terminal = parse(entries[1].1);
    assert_eq!(terminal["errors"][0]["extensions"]["code"], "SOURCE_FAILED");
    assert_default_delivery(&terminal);
}

pub fn assert_multipart_error(response: &HttpResponse) {
    assert_stream_head(
        response,
        "multipart/mixed; boundary=graphql; subscriptionSpec=1.0",
    );
    let entries = multipart_entries(&response.body);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["payload"]["data"]["failingTicks"]["value"], 7);
    assert_default_delivery(&entries[0]["payload"]);
    assert!(entries[1]["payload"].is_null());
    let error = &entries[1]["errors"][0];
    assert_eq!(error["extensions"]["code"], "SOURCE_FAILED");
    assert!(error.get("path").is_none());
    assert!(error.get("locations").is_none());
    let text = std::str::from_utf8(&response.body).expect("multipart response UTF-8");
    let error_position = text.find("SOURCE_FAILED").expect("source error entry");
    let close_position = text.rfind("--graphql--").expect("multipart close entry");
    assert!(error_position < close_position);
}

pub fn assert_finite(response: &HttpResponse) {
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some("application/graphql-response+json; charset=utf-8")
    );
    assert_eq!(
        response
            .headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok()),
        Some(response.body.len())
    );
    assert_eq!(parse_bytes(&response.body)["data"]["greeting"], "hello");
}

fn assert_stream_head(response: &HttpResponse, content_type: &str) {
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some(content_type)
    );
    assert!(!response.headers.contains_key("content-length"));
}

pub fn assert_default_delivery(payload: &Value) {
    assert_eq!(
        payload.pointer("/extensions/chigiri/delivery/droppedTotal"),
        Some(&Value::Null),
        "default delivery metadata must be present and explicitly null"
    );
}

pub fn assert_policy_terminal(accept: &str, response: &HttpResponse) {
    let (events, terminal) = if accept == "text/event-stream" {
        assert_stream_head(response, "text/event-stream; charset=utf-8");
        let entries = sse_entries(&response.body);
        assert!(
            entries.len() >= 4,
            "multiple events precede the policy error"
        );
        assert_eq!(entries.last(), Some(&("complete", "")));
        let terminal = parse(entries[entries.len() - 2].1);
        assert_default_delivery(&terminal);
        let events = entries[..entries.len() - 2]
            .iter()
            .map(|entry| parse(entry.1))
            .collect::<Vec<_>>();
        (events, terminal)
    } else {
        assert_stream_head(
            response,
            "multipart/mixed; boundary=graphql; subscriptionSpec=1.0",
        );
        let mut entries = multipart_entries(&response.body);
        assert!(
            entries.len() >= 3,
            "multiple events precede the policy error"
        );
        let terminal = entries.pop().expect("policy transport error");
        assert_eq!(terminal.get("payload"), Some(&Value::Null));
        assert!(terminal["errors"][0].get("path").is_none());
        assert!(terminal["errors"][0].get("locations").is_none());
        assert!(response.body.ends_with(b"--graphql--"));
        let events = entries
            .into_iter()
            .map(|entry| entry["payload"].clone())
            .collect::<Vec<_>>();
        (events, terminal)
    };
    assert_eq!(
        terminal["errors"][0]["extensions"]["code"],
        "SUBSCRIPTION_LIMIT_EXCEEDED"
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["data"]["ticks"]["value"], index + 1);
        assert_eq!(
            event["data"]["ticks"]["padding"].as_str().map(str::len),
            Some(32 * 1024)
        );
        assert_default_delivery(event);
    }
}

fn sse_entries(body: &[u8]) -> Vec<(&str, &str)> {
    let body = std::str::from_utf8(body).expect("SSE response UTF-8");
    body.split("\n\n")
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut lines = entry.lines();
            let event = lines
                .next()
                .and_then(|line| line.strip_prefix("event: "))
                .expect("SSE event line");
            let data = lines
                .next()
                .and_then(|line| line.strip_prefix("data: "))
                .expect("SSE data line");
            (event, data)
        })
        .collect()
}

fn multipart_entries(body: &[u8]) -> Vec<Value> {
    let body = std::str::from_utf8(body).expect("multipart response UTF-8");
    body.split("--graphql\r\n")
        .skip(1)
        .filter_map(|part| {
            let (headers, payload) = part.split_once("\r\n\r\n")?;
            assert_eq!(headers, "Content-Type: application/json");
            let payload = payload
                .split_once("\r\n--graphql--")
                .map_or(payload, |(payload, _)| payload);
            let payload = payload.strip_suffix("\r\n").unwrap_or(payload);
            Some(parse(payload))
        })
        .collect()
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("GraphQL JSON payload")
}

fn parse_bytes(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("GraphQL JSON response")
}
