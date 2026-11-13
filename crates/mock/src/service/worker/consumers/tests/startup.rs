//! Argument-free startup, disabled execution and closed source selection boundaries.

use super::support::{self, Spec};
use crate::events::SourceLimits;
use crate::logging::test_support::SubscriptionLogs;

#[tokio::test]
async fn ev_05_required_graphql_arguments_do_not_enter_setup_and_disabled_bodies_never_run() {
    let _case = crate::vm_gate::test_case();
    let logs = SubscriptionLogs::new();
    let mut disabled = Spec::new(
        "disabled",
        "error('disabled setup')",
        "error('disabled consume')",
    );
    disabled.enabled = false;
    let specs = [
        disabled,
        Spec::new(
            "active",
            "assert(ctx.scope=='consumer_setup' and ctx.request==ctx.null) return ctx.generator.sequence.String({values={'one'},interval_ms=10})",
            "seen += 1 assert(seen==1) assert(ctx.scope=='consumer' and ctx.request==ctx.null) assert(os.time()==123) assert(ctx.event.depth==0 and ctx.event.topic==ctx.null and ctx.event.droppedTotal==ctx.null) ctx.models.Entry:create({id=event,value=event,depth=ctx.event.depth})",
        ),
    ];
    let worker = support::start(&specs, SourceLimits::default(), &logs).unwrap();
    let records = support::terminals(&logs, 2).await;
    assert!(records.iter().all(|r| r["consumer"] == "active"));
    assert_eq!(
        records
            .iter()
            .filter(|r| r["outcome"] == "committed")
            .count(),
        1
    );
    assert_eq!(
        records
            .iter()
            .filter(|r| r["outcome"] == "stopped" && r["cause"] == "completed")
            .count(),
        1
    );
    assert!(support::entry(&worker, "one", "value").is_some());
    support::stop(worker).await;
}

#[test]
fn ev_05_partial_startup_and_caught_invalid_all_topic_inputs_fail_before_activation() {
    let _case = crate::vm_gate::test_case();
    for invalid in [
        "{}",
        "{all_topics=false}",
        "{all_topics=1}",
        "{topic='x',all_topics=true}",
        "{all_topics=true,extra=true}",
        "{all_topics=true,overflow='drop_newest'}",
    ] {
        let logs = SubscriptionLogs::new();
        let body = format!(
            "local select:any=ctx.pubsub.subscribe.String assert(not pcall(function() select({invalid}) end)) return ctx.pubsub.subscribe.String({{topic='valid'}})"
        );
        let specs = [
            Spec::new(
                "first",
                "return ctx.generator.sequence.String({values={'never'},interval_ms=1})",
                "error('partial consumer must not run')",
            ),
            Spec::new("second", &body, ""),
        ];
        let error = support::start(&specs, SourceLimits::default(), &logs)
            .err()
            .unwrap();
        assert_eq!(error.consumer.as_deref(), Some("second"), "{invalid}");
        let records = logs.take();
        assert!(
            !records
                .iter()
                .any(|r| r["outcome"] == "committed" || r["outcome"] == "rolled_back")
        );
        assert!(
            records
                .iter()
                .any(|r| r["consumer"] == "first" && r["outcome"] == "stopped")
        );
    }
}
