//! Verify that soft-limit notices from every hook slot retain their origin through HTTP termination.

use super::*;
use crate::limits::request::LimitKind;
use serde_json::json;

#[tokio::test]
async fn hook_soft_warnings_preserve_slot_and_field_response_paths() {
    let _case = crate::vm_gate::test_case();
    for slot in HookSlot::ALL {
        let fixture = Fixture::new(
            "type A{text:String!} type Query{value:Int!}",
            &json!({"A":[{"text":"x".repeat(4096)}]}).to_string(),
            &[("Query.value", "return 1")],
            &[(
                slot,
                "for i=1,190 do local rows=ctx.models.A:find({}) end return nil",
            )],
        );
        let notices = Arc::new(Mutex::new(Vec::new()));
        let received = notices.clone();
        let scope = HttpScope::new("127.0.0.1:49123".parse().unwrap())
            .with_limit_warnings(move |_, warning| received.lock().unwrap().push(warning));
        let metadata = ApplicationMetadata::default();
        let headers = [RequestHeader::new("content-type", b"application/json")];
        let request = TransportRequest::new("POST", "/graphql", None, &headers, None, &metadata);
        let response = scope
            .run(fixture.endpoint.handle(
                request,
                BufferedBody::new(br#"{"query":"{alias:value}"}"#.to_vec()),
                &Clock,
            ))
            .await;
        assert_eq!(
            response.status,
            200,
            "{}: {}",
            slot.name(),
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
            json!({"data":{"alias":1}})
        );
        {
            let notices = notices.lock().unwrap();
            let work: Vec<_> = notices
                .iter()
                .filter(|notice| notice.kind == LimitKind::Work)
                .collect();
            assert_eq!(work.len(), 1, "{}: {notices:?}", slot.name());
            assert_eq!(work[0].origin.phase, slot.name());
            if matches!(slot, HookSlot::FieldBefore | HookSlot::FieldAfter) {
                assert_eq!(work[0].origin.coordinate.as_deref(), Some("Query.value"));
                assert_eq!(
                    work[0].origin.path.as_ref(),
                    &[chigiri::PathSegment::from("alias")]
                );
            } else {
                assert!(work[0].origin.coordinate.is_none());
                assert!(work[0].origin.path.is_empty());
            }
        }
        assert!(scope.0.execution_budget.failure().is_none());
        fixture.stop().await;
    }
}
