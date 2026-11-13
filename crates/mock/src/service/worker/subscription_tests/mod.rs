//! Exercise subscription setup through the real bounded worker queue.

mod active_sequences;
mod invalid;
mod sequences;
mod support;
mod type_selectors;

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::SubscriptionOverflow;

use self::support::{Fixture, arguments};

#[tokio::test]
async fn setup_returns_only_the_selected_owned_binding_and_preserves_ordinary_contexts() {
    let _case = crate::vm_gate::test_case();
    let body = r#"
local dynamic: any = ctx
assert(ctx.scope == "subscription_setup")
assert(dynamic.pubsub.publish == nil)
local ignored = ctx.pubsub.subscribe.String({topic = "ignored"})
if args.drop then
    return ctx.pubsub.subscribe.String({topic = args.topic, overflow = "drop_oldest"})
end
return ctx.pubsub.subscribe.String({topic = args.topic})
"#;
    let mut fixture = Fixture::new(body);
    let schema_owners = Arc::strong_count(fixture.worker.client.ir());

    let default = fixture.setup(arguments("team:one", false)).await.unwrap();
    assert_eq!(default.script(), fixture.subscription_script);
    assert_eq!(default.topic().unwrap(), "team:one");
    assert_eq!(default.overflow(), SubscriptionOverflow::Backpressure);

    let dropping = fixture.setup(arguments("team:two", true)).await.unwrap();
    assert_eq!(dropping.script(), fixture.subscription_script);
    assert_eq!(dropping.topic().unwrap(), "team:two");
    assert_eq!(dropping.overflow(), SubscriptionOverflow::DropOldest);
    assert_eq!(Arc::strong_count(fixture.worker.client.ir()), schema_owners);

    assert!(
        fixture
            .worker
            .client
            .execute(fixture.query, Default::default(), None)
            .await
            .is_ok()
    );
    assert!(
        fixture
            .worker
            .client
            .execute(fixture.mutation, Default::default(), None)
            .await
            .is_ok()
    );
    fixture
        .worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn generated_not_implemented_binding_records_the_business_failure() {
    let _case = crate::vm_gate::test_case();
    let mut fixture = Fixture::generated();
    let budget = crate::runtime::ExecutionBudget::default();
    let error = match fixture
        .setup_with_budget(arguments("unused", false), budget.clone())
        .await
    {
        Ok(_) => panic!("generated setup must reject"),
        Err(error) => error,
    };
    assert_eq!(error.phase, "business");
    let failure = budget.business().expect("business failure");
    assert_eq!(failure.code, "NOT_IMPLEMENTED");
    fixture
        .worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}
