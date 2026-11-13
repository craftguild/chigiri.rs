//! Local time overrides are discarded on every unsuccessful request path.

use super::*;
use crate::hooks::HookSlot::{FieldBefore, OperationBefore, RequestAfter};
use serde_json::json;

const SDL: &str = "type Query{now:Int! bad:Int!}";
const PREPARE: &str = "if ctx.operation.name=='Fail' then timeleap.travel_to(200) end return nil";

async fn failed_then_recovers(resolvers: &[(&str, &str)], hooks: &[(HookSlot, &str)]) {
    let fixture = Fixture::new_with_clock(
        SDL,
        "{}",
        resolvers,
        hooks,
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    let (failed, _) = fixture.query("query Fail { now bad }").await;
    assert!(
        failed["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty())
    );
    assert_eq!(
        fixture.query("query Normal { now }").await,
        (json!({"data":{"now":100}}), None)
    );
    fixture.stop().await;
}

#[tokio::test]
async fn tl_10_rejection_invalid_output_later_root_and_after_discard_local_time() {
    let _case = crate::vm_gate::test_case();
    let ordinary = &[
        ("Query.now", "return assert(os.time())"),
        ("Query.bad", "return 1"),
    ];
    failed_then_recovers(
        ordinary,
        &[
            (OperationBefore, PREPARE),
            (
                FieldBefore,
                "if ctx.field.coordinate=='Query.bad' then ctx.reject({code=403}) end return nil",
            ),
        ],
    )
    .await;
    failed_then_recovers(
        ordinary,
        &[(
            OperationBefore,
            "if ctx.operation.name=='Fail' then timeleap.travel_to(200); local invalid:any={unknown=true}; return invalid end return nil",
        )],
    )
    .await;
    failed_then_recovers(
        &[
            ("Query.now", "return assert(os.time())"),
            ("Query.bad", "error('later root failed')"),
        ],
        &[(OperationBefore, PREPARE)],
    )
    .await;
    failed_then_recovers(
        ordinary,
        &[
            (
                OperationBefore,
                "if ctx.operation.name=='Fail' then timeleap.travel_to(200); return {statePatch={count=99}} end return nil",
            ),
            (
                RequestAfter,
                "if ctx.state.count==99 then error('after failed') end return nil",
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn tl_10_timeout_and_cancellation_discard_local_time_and_recover() {
    let _case = crate::vm_gate::test_case();
    let resolvers = &[
        ("Query.now", "return assert(os.time())"),
        ("Query.bad", "while true do end return 1"),
    ];
    failed_then_recovers(resolvers, &[(OperationBefore, PREPARE)]).await;

    let fixture = Fixture::new_with_clock(
        SDL,
        "{}",
        resolvers,
        &[(OperationBefore, PREPARE)],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    let mut pending = Box::pin(fixture.query("query Fail { bad }"));
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        result = &mut pending => panic!("cancellation fixture completed early: {result:?}"),
    }
    drop(pending);
    let recovered = tokio::time::timeout(
        Duration::from_secs(2),
        fixture.query("query Normal { now }"),
    )
    .await
    .expect("cancelled request did not release the worker");
    assert_eq!(recovered, (json!({"data":{"now":100}}), None));
    fixture.stop().await;
}
