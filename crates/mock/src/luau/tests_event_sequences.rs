//! Each sequence element is one complete output for its selected field.

use super::tests_event_sources::{contract, id};
use super::{ScriptCandidate, render_module, render_subscription_module};

#[test]
fn ev_01_sequence_inputs_preserve_scalar_null_list_and_record_types() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for (field, values) in [
        ("count", "{1, 2}"),
        ("maybe", "{ctx.null, 1}"),
        ("texts", "{{'one'}, {'two', 'three'}}"),
        ("item", "{{id='one',title='sample'}}"),
    ] {
        let id = id(&contract, &format!("Signals.{field}"));
        let selector = crate::events::payload::field_type_name(
            contract.ir(),
            contract.ir().scripts()[id.0].field,
        );
        let source = render_subscription_module(
            contract.ir(),
            id,
            &format!("local generate = ctx.generator.sequence.{selector}\nreturn generate({{values={values}, interval_ms=1000, loop=true, overflow='drop_oldest'}})"),
            None,
        )
        .unwrap();
        contract
            .script(ScriptCandidate { script: id, source })
            .unwrap();
    }
}

#[test]
fn ev_01_sequence_inputs_and_roles_reject_wrong_field_and_payloads() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for (field, body) in [
        (
            "count",
            "return ctx.generator.sequence.Item({values={{id='x',title='wrong'}},interval_ms=1})",
        ),
        (
            "count",
            "return ctx.generator.sequence.Int({values={ctx.null},interval_ms=1})",
        ),
        (
            "texts",
            "return ctx.generator.sequence.String({values={'one'},interval_ms=1})",
        ),
        (
            "count",
            "return ctx.generator.sequence.Int({values={1},interval_ms='1'})",
        ),
        (
            "count",
            "return ctx.generator.sequence.Int({values={1},interval_ms=1,loop=1})",
        ),
    ] {
        let id = id(&contract, &format!("Signals.{field}"));
        let source = render_subscription_module(contract.ir(), id, body, None).unwrap();
        assert!(
            contract
                .script(ScriptCandidate { script: id, source })
                .is_err(),
            "{body}"
        );
    }
    for coordinate in ["Read.ping", "Write.emit"] {
        let id = id(&contract, coordinate);
        let source = render_module(
            contract.ir(),
            id,
            "ctx.generator.sequence.Int({values={1},interval_ms=1}) return 1",
        )
        .unwrap();
        assert!(
            contract
                .script(ScriptCandidate { script: id, source })
                .is_err(),
            "{coordinate}"
        );
    }
}
