//! Real short VMs capture definitions and preserve sticky setup rejection.

use std::sync::Arc;

use chigiri::core::{Object, Value};

use super::support::{Fixture, arguments, stop};
use crate::output::{ObjectData, OutputValue};
use crate::value::ValueBudget;

#[tokio::test]
async fn ev_01_sequence_capture_is_immutable_after_setup_vm_destruction() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        r#"
local values = {args.topic, "last"}
local unused = ctx.generator.sequence.String({values={"unused"},interval_ms=1})
local source = ctx.generator.sequence.String({values=values,interval_ms=86400000,loop=true,overflow="drop_oldest"})
values[1] = "mutated"
values[2] = "also mutated"
return source
"#,
    );
    let owners = Arc::strong_count(fixture.worker.client.ir());
    let binding = fixture.setup(arguments("captured", false)).await.unwrap();
    assert_eq!(binding.script(), fixture.subscription_script);
    assert!(binding.topic().is_none());
    assert_eq!(
        binding.overflow(),
        chigiri::SubscriptionOverflow::DropOldest
    );
    let definition = binding.sequence().unwrap().clone();
    assert_eq!(definition.interval_ms(), 86_400_000);
    assert!(definition.looping());
    assert_eq!(definition.len(), 2);
    assert!(definition.bytes() > 0);
    assert!(definition.event_bytes(0).unwrap() <= definition.bytes());
    assert_eq!(Arc::strong_count(fixture.worker.client.ir()), owners);
    let snapshot = crate::store::Store::empty(fixture.worker.client.ir().clone()).snapshot();
    let field = fixture.subscription;
    drop(binding);
    stop(fixture).await;
    for (index, expected) in ["captured", "last"].into_iter().enumerate() {
        let output = definition
            .project(index, snapshot.clone(), field, &mut ValueBudget::default())
            .unwrap();
        assert!(matches!(output, OutputValue::Scalar(Value::String(value)) if value == expected));
    }
    assert!(
        definition
            .project(2, snapshot, field, &mut ValueBudget::default())
            .is_err()
    );
}

#[tokio::test]
async fn ev_01_sequence_complete_outputs_keep_null_list_and_record_provenance() {
    let _case = crate::vm_gate::test_case();
    for (field, values, seed) in [
        ("count", "{1}", None),
        ("maybe", "{ctx.null}", None),
        ("texts", "{{'one','two'}}", None),
        (
            "item",
            "ctx.models.Item:find({})",
            Some(r#"{"Item":[{"id":"one","title":"record"}]}"#),
        ),
        ("dto", "{{value='dto'}}", None),
    ] {
        let selector = match field {
            "count" | "maybe" => "Int",
            "texts" => "String",
            "item" => "Item",
            "dto" => "Payload",
            _ => unreachable!(),
        };
        let fixture = Fixture::for_field(
            &format!("return ctx.generator.sequence.{selector}({{values={values},interval_ms=1}})"),
            field,
            seed,
        );
        let binding = fixture
            .setup(Object::new())
            .await
            .unwrap_or_else(|error| panic!("{field} {values}: {error:?}"));
        let definition = binding.sequence().unwrap();
        assert!(!definition.looping());
        assert_eq!(
            binding.overflow(),
            chigiri::SubscriptionOverflow::Backpressure
        );
        let snapshot = crate::store::Store::empty(fixture.worker.client.ir().clone()).snapshot();
        let output = definition
            .project(
                0,
                snapshot,
                fixture.subscription,
                &mut ValueBudget::default(),
            )
            .unwrap();
        match (field, output) {
            ("count", OutputValue::Scalar(Value::Int(value))) => assert_eq!(value, 1),
            ("maybe", OutputValue::Null) => {}
            ("texts", OutputValue::List(values)) => assert_eq!(values.len(), 2),
            ("item", OutputValue::Object(value)) => {
                assert!(matches!(value.data(), ObjectData::CapturedRecord(_)))
            }
            ("dto", OutputValue::Object(value)) => {
                assert!(matches!(value.data(), ObjectData::Dto(_)))
            }
            _ => panic!("complete captured output shape changed"),
        }
        stop(fixture).await;
    }
}

#[tokio::test]
async fn ev_01_matching_ids_do_not_grant_sequence_record_provenance() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::for_field(
        "return ctx.generator.sequence.Item({values={{id='one',title='record'}},interval_ms=1})",
        "item",
        Some(r#"{"Item":[{"id":"one","title":"record"}]}"#),
    );
    assert!(fixture.setup(Object::new()).await.is_err());
    stop(fixture).await;
}

#[tokio::test]
async fn ev_04_invalid_sequence_inputs_remain_sticky_through_catches() {
    let _case = crate::vm_gate::test_case();
    for input in [
        "{values={},interval_ms=1}",
        "{values={[2]='hole'},interval_ms=1}",
        "{values={a='mixed'},interval_ms=1}",
        "{values={'valid'},interval_ms=0}",
        "{values={'valid'},interval_ms=1.5}",
        "{values={'valid'},interval_ms=86400001}",
        "{values={'valid'},interval_ms='1'}",
        "{values={'valid'},interval_ms=1,loop=1}",
        "{values={'valid'},interval_ms=1,overflow='drop_newest'}",
        "{values={'valid'},interval_ms=1,extra=true}",
        "{values={ctx.null},interval_ms=1}",
    ] {
        let fixture = Fixture::new(&format!(
            r#"
if args.drop then
    local input:any = {input}
    local select:any = ctx.generator.sequence.String
    local ok = pcall(function() select(input) end)
    assert(not ok)
end
return ctx.pubsub.subscribe.String({{topic=args.topic}})
"#
        ));
        assert!(
            fixture.setup(arguments("blocked", true)).await.is_err(),
            "{input}"
        );
        assert_eq!(
            fixture
                .setup(arguments("recovered", false))
                .await
                .unwrap()
                .topic(),
            Some("recovered")
        );
        stop(fixture).await;
    }
}

#[tokio::test]
async fn ev_04_sequence_value_count_has_an_inclusive_thousand_item_bound() {
    let _case = crate::vm_gate::test_case();
    let fixture = Fixture::new(
        r#"
local values = {}
for index=1,1000 do values[index] = "v" end
if args.drop then values[1001] = "over" end
return ctx.generator.sequence.String({values=values,interval_ms=1})
"#,
    );
    assert_eq!(
        fixture
            .setup(arguments("unused", false))
            .await
            .unwrap()
            .sequence()
            .unwrap()
            .len(),
        1_000
    );
    assert!(fixture.setup(arguments("unused", true)).await.is_err());
    stop(fixture).await;
}
