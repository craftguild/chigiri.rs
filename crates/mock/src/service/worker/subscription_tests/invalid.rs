//! Reject forged capabilities, malformed bindings, invalid coordinates, and sticky failures.

use chigiri::core::Object;

use super::support::{Fixture, arguments, foreign_execution, stop};

#[tokio::test]
async fn setup_rejects_forged_null_bad_receiver_and_bad_arity_results() {
    let _case = crate::vm_gate::test_case();
    for body in [
        "local forged: any = {} return forged",
        "return (ctx.null :: any)",
        "return (ctx.pubsub.subscribe.String :: any)({}, {topic = args.topic})",
        "return (ctx.pubsub.subscribe.String :: any)()",
        "return (ctx.pubsub.subscribe.String :: any)({topic = args.topic}, true)",
        "local d:any=ctx local ok=pcall(function() d.pubsub.subscribe.other({topic='x'}) end) assert(not ok) return ctx.pubsub.subscribe.String({topic=args.topic})",
        "local d:any=ctx local ok=pcall(function() d.subscription:bind({topic='x'}) end) assert(not ok) return ctx.pubsub.subscribe.String({topic=args.topic})",
        "local source:any=ctx.pubsub.subscribe.String({topic=args.topic}) local ok=pcall(function() source.topic='forged' end) assert(not ok) return source",
    ] {
        let fixture = Fixture::new(body);
        let error = fixture
            .setup(arguments("team:one", false))
            .await
            .err()
            .unwrap();
        assert!(
            matches!(error.phase, "runtime" | "host" | "value"),
            "{body}: {error}"
        );
        stop(fixture).await;
    }
}

#[tokio::test]
async fn setup_rejects_unbounded_or_open_binding_inputs_and_accepts_exact_topic_limit() {
    let _case = crate::vm_gate::test_case();
    let valid = Fixture::new("return ctx.pubsub.subscribe.String({topic = args.topic})");
    assert_eq!(
        valid
            .setup(arguments(&"x".repeat(256), false))
            .await
            .unwrap()
            .topic()
            .unwrap(),
        "x".repeat(256)
    );
    for topic in [String::new(), "x".repeat(257), "bad\ncontrol".to_owned()] {
        assert!(valid.setup(arguments(&topic, false)).await.is_err());
    }
    stop(valid).await;

    for body in [
        "local input: any = {topic=args.topic, extra=true} return (ctx.pubsub.subscribe.String :: any)(input)",
        "local input: any = {topic=args.topic, overflow='drop_newest'} return (ctx.pubsub.subscribe.String :: any)(input)",
        "local input: any = {topic=true} return (ctx.pubsub.subscribe.String :: any)(input)",
    ] {
        let fixture = Fixture::new(body);
        assert!(
            fixture.setup(arguments("team", false)).await.is_err(),
            "{body}"
        );
        stop(fixture).await;
    }
}

#[tokio::test]
async fn protected_binding_failure_is_sticky_but_a_fresh_job_recovers() {
    let _case = crate::vm_gate::test_case();
    let body = r#"
if args.drop then
    local ok = pcall(function()
        return ctx.pubsub.subscribe.String({topic = ""})
    end)
    assert(not ok)
end
return ctx.pubsub.subscribe.String({topic = args.topic})
"#;
    let fixture = Fixture::new(body);
    assert!(fixture.setup(arguments("team", true)).await.is_err());
    assert_eq!(
        fixture
            .setup(arguments("recovered", false))
            .await
            .unwrap()
            .topic()
            .unwrap(),
        "recovered"
    );
    stop(fixture).await;
}

#[tokio::test]
async fn setup_resource_expiry_destroys_the_vm_and_the_same_worker_recovers() {
    let _case = crate::vm_gate::test_case();
    let body = r#"
if args.drop then
    while true do end
end
return ctx.pubsub.subscribe.String({topic = args.topic})
"#;
    let fixture = Fixture::new(body);
    assert!(fixture.setup(arguments("blocked", true)).await.is_err());
    assert_eq!(
        fixture
            .setup(arguments("recovered", false))
            .await
            .unwrap()
            .topic()
            .unwrap(),
        "recovered"
    );
    stop(fixture).await;
}

#[tokio::test]
async fn setup_rejects_missing_arguments_and_non_subscription_coordinates_before_vm_use() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new("return ctx.pubsub.subscribe.String({topic = args.topic})");
    assert!(fixture.setup(Object::new()).await.is_err());
    let result = fixture
        .worker
        .client
        .subscription_setup(
            fixture.query,
            Object::new(),
            None,
            foreign_execution(&fixture.worker.client),
        )
        .await;
    let error = result.err().unwrap();
    assert_eq!(error.phase, "contract");
    stop(fixture).await;
}
