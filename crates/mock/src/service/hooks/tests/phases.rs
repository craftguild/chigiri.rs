//! Verify six-slot ordering, rejection authority, state isolation, and raw HTTP views.

use super::*;
use HookSlot::*;
use serde_json::json;

const SDL: &str = "type Query { value(input: Int = 9): Int @chigiri_mock_resolver }";
const RESOLVERS: &[(&str, &str)] = &[("Query.value", "return ctx.state.count")];

#[tokio::test]
async fn hk_02_04_07_phases_once_and_state_resets_for_each_http_request() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        SDL,
        "{}",
        RESOLVERS,
        &[
            (
                RequestBefore,
                "ctx.log('info','rb',{count=ctx.state.count}) return {statePatch={count=ctx.state.count+10}}",
            ),
            (
                OperationBefore,
                "ctx.log('info','ob',{count=ctx.state.count}) return {statePatch={count=ctx.state.count+20}}",
            ),
            (FieldBefore, "ctx.log('info','fb',{count=ctx.state.count})"),
            (
                FieldAfter,
                "ctx.log('info','fa',{count=ctx.state.count,result=ctx.result})",
            ),
            (
                OperationAfter,
                "ctx.log('info','oa',{count=ctx.state.count,result=ctx.result})",
            ),
            (
                RequestAfter,
                "ctx.log('info','ra',{count=ctx.state.count,result=ctx.result})",
            ),
        ],
    );
    for _ in 0..2 {
        assert_eq!(
            fixture.query("{ value }").await,
            (json!({"data":{"value":31}}), None)
        );
    }
    let logs = fixture.logs();
    for group in logs.as_chunks::<6>().0 {
        assert_eq!(
            group.iter().map(|entry| entry.0).collect::<Vec<_>>(),
            [
                RequestBefore,
                OperationBefore,
                FieldBefore,
                FieldAfter,
                OperationAfter,
                RequestAfter
            ]
        );
        assert_eq!(
            group
                .iter()
                .map(|entry| entry.1["count"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [1, 11, 31, 31, 31, 31]
        );
        assert_eq!(group[3].1["result"], json!({"value":31,"errors":[]}));
        assert_eq!(
            group[4].1["result"],
            json!({"data":{"value":31},"errors":[]})
        );
    }
    assert_eq!(logs.len(), 12);
    {
        let events = fixture.events.lock().unwrap();
        assert_ne!(events[0].request_id, events[6].request_id);
        assert!(
            events[..6]
                .iter()
                .all(|event| event.request_id == events[0].request_id)
        );
        assert_eq!(events[2].coordinate.as_deref(), Some("Query.value"));
        assert_eq!(
            events[2].path.as_ref(),
            [chigiri::PathSegment::from("value")]
        );
    }
    fixture.stop().await;
}

#[tokio::test]
async fn hk_07_10_31_reject_observes_reached_phase_and_never_resumes_next_or_patch() {
    let _case = crate::vm_gate::test_case();
    for (slot, status) in [
        (RequestBefore, 200),
        (OperationBefore, 401),
        (FieldBefore, 403),
    ] {
        let body = format!(
            "pcall(function() ctx.reject({{code={status},message='Denied'}}) end) pcall(function() ctx.reject({{code=511}}) end) return nil"
        );
        let fixture = Fixture::new(
            SDL,
            "{}",
            &[("Query.value", "error('rejected resolver must not run')")],
            &[
                (slot, &body),
                (
                    FieldAfter,
                    "ctx.log('info','fa',{count=ctx.state.count,result=ctx.result})",
                ),
                (
                    OperationAfter,
                    "ctx.log('info','oa',{count=ctx.state.count,result=ctx.result})",
                ),
                (
                    RequestAfter,
                    "ctx.log('info','ra',{count=ctx.state.count,result=ctx.result})",
                ),
            ],
        );
        let (response, override_status) = fixture.query("{ denied:value }").await;
        assert_eq!(override_status, Some(status));
        assert_eq!(response["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
        assert_eq!(response["errors"][0]["extensions"]["httpStatus"], status);
        assert!(response["errors"][0]["extensions"]["requestId"].is_string());
        assert_eq!(response["errors"][0]["message"], "Denied");
        let logs = fixture.logs();
        // Rejection is terminal, so the after hook must not start afterward.
        assert!(logs.is_empty());
        assert_eq!(response["data"], json!(null));
        assert_eq!(response["errors"].as_array().unwrap().len(), 1);
        fixture.stop().await;
    }
}

#[tokio::test]
async fn hk_08_http_snapshot_uses_exact_body_get_duplicates_and_numeric_listener() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        SDL,
        "{}",
        RESOLVERS,
        &[
            (
                RequestBefore,
                "assert(ctx.request.host == '127.0.0.1') assert(ctx.request.port == 49123) ctx.log('info','request',{method=ctx.request.method,url=ctx.request.url,headers=ctx.request.headers,length=#ctx.request.body}) if #ctx.request.body < 4096 then ctx.log('info','body',{body=ctx.request.body}) end",
            ),
            (
                OperationBefore,
                "ctx.log('info','operation',{operation={kind=(ctx.operation.kind :: string),name=ctx.operation.name,variables=ctx.operation.variables}})",
            ),
            (FieldBefore, "ctx.log('info','field',{field=ctx.field})"),
        ],
    );
    let body = b"{  \"query\":\"query Named($n:Int=17) { aliased:value(input:$n) }\" }\n";
    let extra = [
        RequestHeader::new("Host", b"evil.invalid:123"),
        RequestHeader::new("Forwarded", b"host=forged"),
        RequestHeader::new("x-repeat", b"first"),
        RequestHeader::new("X-Repeat", b"second"),
        RequestHeader::new("x-byte", b"\xff"),
        RequestHeader::new("x-literal", br"\xFF"),
    ];
    let (response, status) = fixture
        .request("POST", Some("ignored=1"), body.to_vec(), &extra)
        .await;
    fixture.logs();
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(status, None);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({"data":{"aliased":1}})
    );
    let logs = fixture.logs();
    assert_eq!(logs[0].1["method"], "POST");
    assert_eq!(logs[0].1["url"], "/graphql?ignored=1");
    assert_eq!(logs[1].1["body"], std::str::from_utf8(body).unwrap());
    let headers = logs[0].1["headers"].as_array().unwrap();
    assert_eq!(headers[4], json!({"name":"x-repeat","value":"first"}));
    assert_eq!(headers[5], json!({"name":"X-Repeat","value":"second"}));
    assert_eq!(headers[6]["value"], r"\xFF");
    assert_eq!(headers[7]["value"], r"\\xFF");
    assert_eq!(
        logs[2].1["operation"],
        json!({"kind":"query","name":"Named","variables":{"n":17}})
    );
    assert_eq!(
        logs[3].1["field"],
        json!({"coordinate":"Query.value","path":["aliased"],"arguments":{"input":17},"parent":null})
    );
    fixture.events.lock().unwrap().clear();
    let (response, _) = fixture
        .request("GET", Some("query=%7Bvalue%7D"), b"ignored".to_vec(), &[])
        .await;
    fixture.logs();
    assert_eq!(
        response.status,
        200,
        "{}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(fixture.logs()[1].1["body"], "");
    fixture.events.lock().unwrap().clear();
    let mut large = b"{\"query\":\"{value}\"}".to_vec();
    large.extend(vec![b' '; 5000]);
    let (response, _) = fixture.request("POST", None, large, &[]).await;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap()["data"],
        json!({"value":1})
    );
    assert!(fixture.logs()[0].1["length"].as_u64().unwrap() > 4096);
    fixture.stop().await;
}
