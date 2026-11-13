//! Validate all six closed hook-module types and declared values through real Luau checking and initialization.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::{Contract, HookCandidate, render_hook_module};
use crate::hooks::{HookSlot, StateContract, StateType};
use crate::ir::MockIr;
use crate::value::ValueBudget;

fn contract(source: &str, authenticated: bool) -> Contract {
    let fields = [
        ("authenticated".into(), StateType::Boolean),
        (
            "user".into(),
            StateType::Nullable {
                of: Box::new(StateType::String),
            },
        ),
        (
            "profile".into(),
            StateType::Object {
                fields: [("name".into(), StateType::String)].into(),
            },
        ),
    ]
    .into();
    let initial = Object::from_iter([
        ("authenticated", Value::Boolean(authenticated)),
        (
            "profile",
            Value::Object(Object::from_iter([(
                "name",
                Value::String("résumé 🙂".into()),
            )])),
        ),
    ]);
    let state =
        StateContract::checked(fields, &Value::Object(initial), &mut ValueBudget::default())
            .unwrap();
    Contract::with_state(
        Arc::new(MockIr::parse(vec![Source::new("schema.graphql", source)]).unwrap()),
        state,
    )
    .unwrap()
}

fn candidate(slot: HookSlot, body: &str) -> HookCandidate {
    HookCandidate {
        slot,
        source: render_hook_module(slot, body).unwrap(),
    }
}

#[test]
fn hk_01_03_all_slot_modules_share_typed_state_read_models_and_phase_contracts() {
    let _case = crate::vm_gate::test_case();
    for schema in [
        include_str!("../../fixtures/blog.graphql"),
        include_str!("../../fixtures/projects.graphql"),
    ] {
        let contract = contract(schema, false);
        let bodies = [
            (
                HookSlot::RequestBefore,
                "if not ctx.state.authenticated then ctx.reject({code=401}) end\nctx.log('info',ctx.request.method)\nreturn {statePatch={user=ctx.null}}",
            ),
            (
                HookSlot::RequestAfter,
                "ctx.log('info','observed',{data=ctx.result.data})\nreturn nil",
            ),
            (
                HookSlot::OperationBefore,
                "ctx.log('debug',ctx.operation.kind)\nreturn {statePatch={authenticated=true}}",
            ),
            (
                HookSlot::OperationAfter,
                "ctx.log('warn',ctx.operation.kind,{data=ctx.result.data})\nreturn nil",
            ),
            (
                HookSlot::FieldBefore,
                "ctx.log('info',ctx.field.coordinate,{parent=ctx.field.parent})\nreturn nil",
            ),
            (
                HookSlot::FieldAfter,
                "ctx.log('error',ctx.field.coordinate,{value=ctx.result.value})\nreturn nil",
            ),
        ];
        let scripts = contract
            .hooks(
                bodies
                    .into_iter()
                    .map(|(slot, body)| candidate(slot, body))
                    .collect(),
                None,
            )
            .unwrap();
        for (script, slot) in scripts.iter().zip(HookSlot::ALL) {
            assert_eq!(script.slot(), slot);
            assert_eq!(
                super::format::source(script.source(), super::MAX_BODY).unwrap(),
                script.source()
            );
        }
        for slot in HookSlot::ALL {
            contract
                .hooks(vec![candidate(slot, "return nil")], None)
                .unwrap();
        }
    }
}

#[test]
fn hk_03_phase_types_reject_write_methods_after_request_reject_and_field_patch() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"), false);
    for (slot, body) in [
        (
            HookSlot::RequestBefore,
            "ctx.models.User:create({name='forbidden'})\nreturn nil",
        ),
        (
            HookSlot::OperationBefore,
            "return {statePatch={authenticated='wrong'}}",
        ),
        (HookSlot::RequestBefore, "return {statePatch={profile={}}}"),
        (
            HookSlot::FieldBefore,
            "return {statePatch={authenticated=true}}",
        ),
        (
            HookSlot::FieldBefore,
            "ctx.reject({code='401'})\nreturn nil",
        ),
        (HookSlot::RequestAfter, "ctx.reject({code=401})\nreturn nil"),
        (
            HookSlot::OperationAfter,
            "ctx.log('info',ctx.request.body)\nreturn nil",
        ),
        (
            HookSlot::FieldAfter,
            "return {statePatch={authenticated=true}}",
        ),
        (HookSlot::RequestBefore, "return ctx.state.undeclared"),
        (
            HookSlot::RequestBefore,
            "ctx.log('fatal','wrong')\nreturn nil",
        ),
    ] {
        let error = contract
            .hooks(vec![candidate(slot, body)], None)
            .err()
            .unwrap();
        assert_eq!(error.phase, "type", "{slot:?}: {body}");
        assert_eq!(error.hook, Some(slot));
        assert_eq!(error.script, None);
        assert!(!error.to_string().contains("forbidden"));
    }
}

#[test]
fn tl_08_checker_limits_timeleap_to_preparation_hooks() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"), false);
    for slot in [HookSlot::RequestBefore, HookSlot::OperationBefore] {
        contract
            .hooks(
                vec![candidate(
                    slot,
                    "timeleap.travel_to(1); timeleap.reset(nil); return nil",
                )],
                None,
            )
            .unwrap();
    }
    for slot in [
        HookSlot::RequestAfter,
        HookSlot::OperationAfter,
        HookSlot::FieldBefore,
        HookSlot::FieldAfter,
    ] {
        let error = contract
            .hooks(vec![candidate(slot, "timeleap.reset(); return nil")], None)
            .err()
            .unwrap();
        assert_eq!(error.phase, "type", "{slot:?}: {error:?}");
    }
}

#[test]
fn hk_01_03_hook_initialization_rejects_any_module_shape_without_calling_run() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"), false);
    for slot in HookSlot::ALL {
        for source in [
            "local m:any={} return m",
            "local m:any={state='ready',resolve=function() end} return m",
            "local m:any={state='ready',run=true} return m",
            "local m:any={state='Ready',run=function() end} return m",
            "local m:any={state='ready',run=function() end,extra=true} return m",
            "local m:any=function() end return m",
        ] {
            let error = contract
                .hooks(
                    vec![HookCandidate {
                        slot,
                        source: source.into(),
                    }],
                    None,
                )
                .err()
                .unwrap();
            assert_eq!(error.phase, "module");
            assert_eq!(error.hook, Some(slot));
        }
        let scripts = contract
            .hooks(
                vec![candidate(
                    slot,
                    "error('run must not be called during initialization')",
                )],
                None,
            )
            .unwrap();
        assert_eq!(scripts[0].slot(), slot);
    }
}

#[test]
fn hk_02_state_declarations_depend_on_declared_shapes_and_preserve_schema_digest() {
    let _case = crate::vm_gate::test_case();
    let schema = include_str!("../../fixtures/blog.graphql");
    let first = contract(schema, false);
    let second = contract(schema, true);
    assert_eq!(first.declarations(), second.declarations());
    assert_eq!(
        first.ir().graph().source().digest(),
        second.ir().graph().source().digest()
    );
    let empty = Contract::new(first.ir().clone()).unwrap();
    assert_ne!(first.declarations(), empty.declarations());
    assert_eq!(
        first.ir().graph().source().digest(),
        empty.ir().graph().source().digest()
    );
    assert!(!first.declarations().contains("résumé 🙂"));
    first
        .script(super::tests::candidate(
            &first,
            "Query.getUsers",
            "if ctx.state.authenticated then return ctx.models.User:find({}) end\nreturn {} ",
        ))
        .unwrap();
}

#[test]
fn hk_03_hook_source_capacity_duplicates_and_wrapper_escape_fail_before_initialization() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"), false);
    let mut module = candidate(HookSlot::RequestBefore, "return nil");
    module
        .source
        .push_str(&" ".repeat(super::MAX_BODY - module.source.len()));
    contract.hooks(vec![module.clone()], None).unwrap();
    module.source.push(' ');
    assert_eq!(
        contract.hooks(vec![module], None).err().unwrap().phase,
        "limit"
    );
    let module = candidate(HookSlot::RequestBefore, "return nil");
    assert_eq!(
        contract
            .hooks(vec![module.clone(), module.clone()], None)
            .err()
            .unwrap()
            .phase,
        "contract"
    );
    for source in ["return {}\nend\nlocal function escape()", "return ("] {
        let mut module = module.clone();
        module.source = source.into();
        assert_eq!(
            contract.hooks(vec![module], None).err().unwrap().phase,
            "compile"
        );
    }
}
