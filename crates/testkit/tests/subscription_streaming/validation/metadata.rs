//! Negative wire fixtures distinguish an absent leaf from an explicit JSON null.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};

use super::{HttpResponse, assert_multipart_ticks, assert_sse_ticks};

#[test]
fn present_null_metadata_keeps_both_ordered_wire_validators_valid() {
    for multipart in [false, true] {
        validate(multipart, &response(multipart, Some(Value::Null), 1));
    }
}

#[test]
fn both_wire_validators_reject_missing_or_wrong_metadata_on_each_event() {
    for multipart in [false, true] {
        for changed_event in [0, 1] {
            for leaf in [
                None,
                Some(json!(0)),
                Some(json!(false)),
                Some(json!("null")),
            ] {
                let response = response(multipart, leaf, changed_event);
                assert!(
                    catch_unwind(AssertUnwindSafe(|| validate(multipart, &response))).is_err(),
                    "wire validator accepted missing or incorrect metadata: multipart={multipart}, event={changed_event}"
                );
            }
        }
    }
}

fn validate(multipart: bool, response: &HttpResponse) {
    if multipart {
        assert_multipart_ticks(response);
    } else {
        assert_sse_ticks(response);
    }
}

fn response(multipart: bool, changed_leaf: Option<Value>, changed_event: usize) -> HttpResponse {
    let values = [1, 2].into_iter().enumerate().map(|(index, value)| {
        let mut payload = json!({"data":{"ticks":{"value":value}}});
        let leaf = if index == changed_event {
            changed_leaf.clone()
        } else {
            Some(Value::Null)
        };
        if let Some(leaf) = leaf {
            payload["extensions"] = json!({"chigiri":{"delivery":{"droppedTotal":leaf}}});
        }
        payload
    });
    let (content_type, body) = if multipart {
        let mut body = String::new();
        for value in values {
            body.push_str("--graphql\r\nContent-Type: application/json\r\n\r\n");
            body.push_str(&json!({"payload":value}).to_string());
            body.push_str("\r\n");
        }
        body.push_str("--graphql--");
        (
            "multipart/mixed; boundary=graphql; subscriptionSpec=1.0",
            body,
        )
    } else {
        let mut body = String::new();
        for value in values {
            body.push_str(&format!("event: next\ndata: {value}\n\n"));
        }
        body.push_str("event: complete\ndata: \n\n");
        ("text/event-stream; charset=utf-8", body)
    };
    HttpResponse {
        status: 200,
        headers: BTreeMap::from([("content-type".to_owned(), content_type.to_owned())]),
        body: body.into_bytes(),
    }
}
