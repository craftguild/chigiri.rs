//! Preserve the highest valid sibling rejection even when non-null propagation drops its reply.

use super::scheduling::{Pause, Schedule, Stage};
use super::*;
use serde_json::json;

#[tokio::test]
async fn reached_sibling_rejections_choose_the_maximum_status_in_response_order() {
    let _case = crate::vm_gate::test_case();
    for order in [[0, 1], [1, 0]] {
        for first in [0, 1] {
            let before = [
                Pause::new("a", Stage::BeforeHook),
                Pause::new("b", Stage::BeforeHook),
            ];
            let ready = [
                Pause::new("a", Stage::HookResult),
                Pause::new("b", Stage::HookResult),
            ];
            let after = Pause::new(["a", "b"][first], Stage::AfterHook);
            let fixture = Fixture::new(
                "type Query{value(requested:Int!):Int!}",
                "{}",
                &[("Query.value", "error('resolver must not run')")],
                &[(
                    HookSlot::FieldBefore,
                    "if ctx.field.arguments.requested == 200 then ctx.reject({code=200,message='first'}) else ctx.reject({code=403,message='second'}) end return nil",
                )],
            )
            .scheduled(Schedule::new(&[
                before[0].clone(),
                before[1].clone(),
                ready[0].clone(),
                ready[1].clone(),
                after.clone(),
            ]));
            let (response, ()) = tokio::join!(
                fixture.request(
                    "POST",
                    None,
                    serde_json::to_vec(
                        &json!({"query":"{a:value(requested:200) b:value(requested:403)}"})
                    )
                    .unwrap(),
                    &[]
                ),
                async {
                    before[0].reached().await;
                    before[1].reached().await;
                    for index in order {
                        before[index].proceed().await;
                        ready[index].reached().await;
                    }
                    ready[first].proceed().await;
                    after.reached().await;
                    after.proceed().await;
                }
            );
            assert_eq!(response.0.status, 403, "VM sequence is equal to {order:?}");
            let body: serde_json::Value = serde_json::from_slice(&response.0.body).unwrap();
            assert_eq!(body["data"], json!(null));
            assert_eq!(body["errors"][0]["extensions"]["code"], "MOCK_REJECTED");
            assert_eq!(body["errors"][0]["extensions"]["httpStatus"], 403);
            assert_eq!(body["errors"][0]["message"], "second");
            fixture.stop().await;
        }
    }
}
