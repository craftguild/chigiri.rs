//! Preserve business-event continuation while terminating typed host failures.

use chigiri::__streaming::SubscriptionOutput;
use std::sync::atomic::Ordering;

use super::support::{Fixture, next, output, success};
use crate::hooks::HookSlot;

#[tokio::test]
async fn business_event_remains_all_null_and_the_next_event_continues() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture.subscribe_query(
        "subscription { chosen: changed(topic:\"events\") { id value current(mode:\"business\") } }"
    ).await;
    success(
        &fixture
            .execute("mutation { publish(id:\"business\") { id } }")
            .await,
    );
    let SubscriptionOutput::Event(response) = output(&mut subscription.live).await else {
        panic!("business failure terminated the source")
    };
    assert_eq!(response.data, Some(chigiri::Value::Null));
    assert_eq!(response.errors[0].message, "event denied");
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&chigiri::Value::from("DENIED"))
    );
    assert_eq!(next(&mut subscription.live).await["chosen"]["value"], 2);
    subscription.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn internal_event_terminates_and_the_same_worker_supports_new_requests_and_sources() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new();
    let mut subscription = fixture.subscribe_query(
        "subscription { chosen: changed(topic:\"events\") { id value current(mode:\"internal\") } }"
    ).await;
    success(
        &fixture
            .execute("mutation { publish(id:\"internal\") { id } }")
            .await,
    );
    let SubscriptionOutput::Terminal(response) = output(&mut subscription.live).await else {
        panic!("internal event failure did not terminate immediately")
    };
    assert!(response.data.is_none());
    assert!(!format!("{response:?}").contains("private event failure"));
    assert_eq!(fixture.reports.load(Ordering::SeqCst), 1);
    {
        let diagnostics = fixture.resolver_diagnostics.lock().unwrap();
        assert_eq!(
            diagnostics.len(),
            1,
            "{}",
            serde_json::to_string(&*diagnostics).unwrap()
        );
    }
    assert!(matches!(
        output(&mut subscription.live).await,
        SubscriptionOutput::Cancelled
    ));
    subscription.close().await;
    success(&fixture.execute("{ ping }").await);
    let mut later = fixture.subscribe().await;
    success(
        &fixture
            .execute("mutation { publish(id:\"recovered\") { id } }")
            .await,
    );
    assert_eq!(next(&mut later.live).await["chosen"]["id"], "recovered");
    later.close().await;
    fixture.stop().await;
}

#[tokio::test]
async fn request_operation_and_field_hook_failures_take_the_immediate_terminal_path() {
    let _case = crate::vm_gate::test_case();
    for slot in [
        HookSlot::RequestBefore,
        HookSlot::OperationBefore,
        HookSlot::FieldBefore,
        HookSlot::RequestAfter,
    ] {
        let body = if slot == HookSlot::RequestAfter {
            "local data:any=ctx.result.data if type(data)=='table' and data.chosen~=nil and ctx.models.Item:count({where={field='id',op='eq',value='bad'}})>0 then error('private hook failure') end"
        } else {
            "if ctx.request.body=='' and ctx.models.Item:count({where={field='id',op='eq',value='bad'}})>0 then error('private hook failure') end"
        };
        let fixture = Fixture::with_hooks(&[(slot, body)]);
        let mut subscription = fixture.subscribe().await;
        success(
            &fixture
                .execute("mutation { publish(id:\"bad\") { id } }")
                .await,
        );
        let SubscriptionOutput::Terminal(response) = output(&mut subscription.live).await else {
            panic!("{slot:?} internal failure was not terminal")
        };
        assert!(response.data.is_none());
        assert!(!format!("{response:?}").contains("private hook failure"));
        assert_eq!(fixture.reports.load(Ordering::SeqCst), 0, "{slot:?}");
        assert_eq!(
            fixture.hook_diagnostics.load(Ordering::SeqCst),
            1,
            "{slot:?}"
        );
        subscription.close().await;
        success(&fixture.execute("{ ping }").await);
        success(&fixture.execute("mutation { clear }").await);
        let mut later = fixture.subscribe().await;
        success(
            &fixture
                .execute("mutation { publish(id:\"good\") { id } }")
                .await,
        );
        assert_eq!(next(&mut later.live).await["chosen"]["id"], "good");
        later.close().await;
        fixture.stop().await;
    }
}
