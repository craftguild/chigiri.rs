//! Resolver request snapshots are available without hooks and stay immutable per request.

use super::*;
use chigiri_http::RequestHeader;
use serde_json::json;

const SDL: &str = r#"
    type Item { id: ID! label: String! @virtual }
    type Query {
        request: String! @chigiri_mock_resolver
        item: Item! @chigiri_mock_resolver
    }
"#;

const REQUEST: &str = r#"
    local raw = ctx.request
    if raw == ctx.null then return "none" end
    local request = raw :: ChigiriHookRequest
    local values = {}
    for _, header in ipairs(request.headers) do
        if string.lower(header.name) == "x-repeat" or header.name == "x-byte" then
            table.insert(values, header.value)
        end
    end
    return table.concat(values, "|") .. "@" .. request.url
"#;

const LABEL: &str = r#"
    local raw = ctx.request
    if raw == ctx.null then return "none" end
    local request = raw :: ChigiriHookRequest
    return request.method .. "@" .. request.host .. ":" .. tostring(request.port)
"#;

#[tokio::test]
async fn sa_04_hooks_disabled_root_and_nested_resolvers_share_bounded_request_snapshot() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        SDL,
        r#"{"Item":[{"id":"one"}]}"#,
        &[
            ("Query.request", REQUEST),
            ("Query.item", "return ctx.models.Item:find({})[1]"),
            ("Item.label", LABEL),
        ],
        &[],
    );
    let extras = [
        RequestHeader::new("x-repeat", b"first"),
        RequestHeader::new("X-Repeat", b"second"),
        RequestHeader::new("x-byte", b"\xff"),
    ];
    let (response, status) = fixture
        .request(
            "POST",
            None,
            br#"{"query":"{request item{label}}"}"#.to_vec(),
            &extras,
        )
        .await;
    assert_eq!(status, None);
    assert_eq!(response.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(
        body,
        json!({"data": {
            "request": "first|second|\\xFF@/graphql",
            "item": {"label": "POST@127.0.0.1:49123"}
        }})
    );

    let changed = [RequestHeader::new("x-repeat", b"third")];
    let (response, _) = fixture
        .request("POST", None, br#"{"query":"{request}"}"#.to_vec(), &changed)
        .await;
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["data"]["request"], "third@/graphql");
    fixture.stop().await;
}

#[tokio::test]
async fn sa_04_request_tables_are_deep_readonly_and_failure_does_not_poison_next_vm() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        "type Query { bad: String! @chigiri_mock_resolver ok: String! @chigiri_mock_resolver }",
        "{}",
        &[
            (
                "Query.bad",
                "local raw=ctx.request if raw==ctx.null then return 'none' end local request=raw :: ChigiriHookRequest request.headers[1].value='changed' return request.headers[1].value",
            ),
            ("Query.ok", "return 'ok'"),
        ],
        &[],
    );
    let (failed, _) = fixture.query("{bad}").await;
    assert!(failed["errors"].is_array());
    assert_eq!(fixture.query("{ok}").await.0, json!({"data":{"ok":"ok"}}));
    fixture.stop().await;
}
