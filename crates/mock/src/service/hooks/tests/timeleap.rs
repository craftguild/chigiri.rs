//! Request-local and host-owned world time across the real hook/HTTP pipeline.

use super::*;
use crate::hooks::HookSlot::*;
use serde_json::json;

const SDL: &str = "type Query { now: Int! @chigiri_mock_resolver }";
const RESOLVER: &[(&str, &str)] = &[("Query.now", "return assert(os.time())")];

#[tokio::test]
async fn tl_03_local_defaults_share_one_request_view_and_do_not_publish() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new_with_clock(
        SDL,
        "{}",
        RESOLVER,
        &[(
            RequestBefore,
            "timeleap.travel_to(101) timeleap.travel_to(102,nil) timeleap.travel_to(103,{}) timeleap.travel_to(104,{world=false})",
        )],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    assert_eq!(
        fixture.query("{ now }").await,
        (json!({"data":{"now":104}}), None)
    );
    assert_eq!(
        fixture.query("{ now }").await,
        (json!({"data":{"now":104}}), None)
    );
    assert_eq!(
        fixture.worker.control().policy(),
        crate::clock::Policy::Fixed(100)
    );
    fixture.stop().await;
}

#[tokio::test]
async fn tl_06_local_reset_uses_capture_and_world_reset_uses_startup() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new_with_clock(
        SDL,
        "{}",
        RESOLVER,
        &[(RequestBefore, "timeleap.travel_to(200) timeleap.reset()")],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    let control = fixture.worker.control();
    control
        .run_luau("timeleap.travel_to(300, {world=true})")
        .unwrap();
    assert_eq!(
        fixture.query("{ now }").await,
        (json!({"data":{"now":300}}), None)
    );
    control.run_luau("timeleap.reset({world=true})").unwrap();
    assert_eq!(
        fixture.query("{ now }").await,
        (json!({"data":{"now":100}}), None)
    );
    fixture.stop().await;
}

#[tokio::test]
async fn tl_08_world_authority_and_invalid_options_are_sticky_in_request_hooks() {
    let _case = crate::vm_gate::test_case();
    for call in [
        "timeleap.travel_to(200,{world=true})",
        "timeleap.travel_to(200,({global=false} :: any))",
        "timeleap.travel_to(200,({scope='request'} :: any))",
        "timeleap.reset(({world=1} :: any))",
    ] {
        let body = format!("pcall(function() {call} end)");
        let fixture = Fixture::new_with_clock(
            SDL,
            "{}",
            RESOLVER,
            &[(RequestBefore, &body)],
            crate::clock::ClockSettings::fixed(100).unwrap(),
        );
        let (response, _) = fixture.query("{ now }").await;
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "MOCK_EXECUTION_FAILED",
            "{call}: {response}"
        );
        assert_eq!(
            fixture.worker.control().policy(),
            crate::clock::Policy::Fixed(100)
        );
        fixture.stop().await;
    }
}

#[tokio::test]
async fn tl_05_request_view_is_shared_across_hooks_and_root_vms() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new_with_clock(
        "type Item{id:ID! observed:Int! @virtual} type Query{now:Int! item:Item!}",
        r#"{"Item":[{"id":"one"}]}"#,
        &[
            ("Query.now", "return assert(os.time())"),
            ("Query.item", "return ctx.models.Item:find({})[1]"),
            ("Item.observed", "return assert(os.time())"),
        ],
        &[
            (RequestBefore, "timeleap.travel_to(150)"),
            (
                OperationBefore,
                "assert(os.time()==150); timeleap.travel_to(160)",
            ),
        ],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    assert_eq!(
        fixture
            .query("query Shared { first: now item { observed } second: now }")
            .await,
        (
            json!({"data":{"first":160,"item":{"observed":160},"second":160}}),
            None
        )
    );
    assert_eq!(
        fixture.worker.control().policy(),
        crate::clock::Policy::Fixed(100)
    );
    fixture.stop().await;
}

#[tokio::test]
async fn tl_10_failed_local_preparation_is_discarded_before_next_request() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new_with_clock(
        SDL,
        "{}",
        RESOLVER,
        &[(
            OperationBefore,
            "if ctx.operation.name=='Fail' then timeleap.travel_to(200); error('x') end",
        )],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    let (failed, _) = fixture.query("query Fail { now }").await;
    assert_eq!(
        failed["errors"][0]["extensions"]["code"],
        "MOCK_EXECUTION_FAILED"
    );
    assert_eq!(
        fixture.query("query Normal { now }").await,
        (json!({"data":{"now":100}}), None)
    );
    fixture.stop().await;
}

#[tokio::test]
async fn tl_08_runtime_aliases_cannot_mutate_outside_preparation() {
    let _case = crate::vm_gate::test_case();
    for (resolvers, hooks) in [
        (
            &[(
                "Query.now",
                "local leap:any=timeleap; leap.travel_to(200); return assert(os.time())",
            )][..],
            &[][..],
        ),
        (
            RESOLVER,
            &[(
                FieldBefore,
                "local global:any=_G; local leap:any=global.timeleap; leap.travel_to(200); return nil",
            )][..],
        ),
        (
            RESOLVER,
            &[(
                RequestAfter,
                "local global:any=_G; global['timeleap'].reset(); return nil",
            )][..],
        ),
    ] {
        let fixture = Fixture::new_with_clock(
            SDL,
            "{}",
            resolvers,
            hooks,
            crate::clock::ClockSettings::fixed(100).unwrap(),
        );
        let (response, _) = fixture.query("{ now }").await;
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "MOCK_EXECUTION_FAILED",
            "{response}"
        );
        assert_eq!(
            fixture.worker.control().policy(),
            crate::clock::Policy::Fixed(100)
        );
        fixture.stop().await;
    }
}

#[tokio::test]
async fn tl_16_timeleap_reference_examples_execute_in_their_published_phases() {
    let _case = crate::vm_gate::test_case();
    let selection = crate::reference::Selection::new(
        Some(crate::reference::Topic::Runtime),
        Some("timeleap".into()),
    )
    .unwrap();
    let bytes = crate::reference::ReferenceDocument::build(None)
        .unwrap()
        .render_selection(crate::reference::Format::Json, &selection)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let examples = value["content"]["items"][0]["examples"].as_array().unwrap();
    let example = |title: &str| {
        examples
            .iter()
            .find(|example| example["title"] == title)
            .unwrap()["code"]
            .as_str()
            .unwrap()
    };
    let fixture = Fixture::new_with_clock(
        "type Query { renamedNow: Int! @chigiri_mock_resolver }",
        "{}",
        &[("Query.renamedNow", "return assert(os.time())")],
        &[(RequestBefore, example("Request preparation"))],
        crate::clock::ClockSettings::fixed(100).unwrap(),
    );
    assert_eq!(
        fixture.query("{ renamedNow }").await,
        (json!({"data":{"renamedNow":100}}), None)
    );
    fixture
        .worker
        .control()
        .run_luau(example("Dedicated host-owned controller"))
        .unwrap();
    assert_eq!(
        fixture.query("{ renamedNow }").await,
        (json!({"data":{"renamedNow":100}}), None)
    );
    fixture.stop().await;
}
