//! Verifies that hooks using `any` still obey readonly state, atomic patches,
//! and sticky rejection at the same execution boundary.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chigiri::core::{Object, Source, Value};

use super::{HookCall, HookLog, HookOutcome, execute_hook};
use crate::cancellation::Cancellation;
use crate::hooks::{HookSlot, RequestState, StateContract};
use crate::ir::MockIr;
use crate::luau::{Contract, HookCandidate, HookScript};
use crate::store::Store;
use crate::value::ValueBudget;

pub(super) fn contract() -> Contract {
    let state = StateContract::from_json(r#"{
        "fields": {
            "authenticated":{"kind":"boolean"}, "name":{"kind":"string"},
            "count":{"kind":"integer"}, "number":{"kind":"number"},
            "profile":{"kind":"object","fields":{"name":{"kind":"string"},"tag":{"kind":"nullable","of":{"kind":"string"}}}},
            "list":{"kind":"list","of":{"kind":"object","fields":{}}},
            "user":{"kind":"nullable","of":{"kind":"string"}}
        },
        "initial":{"authenticated":false,"name":"Example","count":0,"number":1.5,"profile":{"name":"Original"},"list":[]}
    }"#, &mut ValueBudget::default()).unwrap();
    Contract::with_state(
        Arc::new(
            MockIr::parse(vec![Source::new(
                "hook.graphql",
                include_str!("../../fixtures/blog.graphql"),
            )])
            .unwrap(),
        ),
        state,
    )
    .unwrap()
}

pub(super) fn script(contract: &Contract, slot: HookSlot, body: &str) -> HookScript {
    contract
        .hooks(
            vec![HookCandidate {
                slot,
                source: format!(
                    "local module: any = {{ run=function(ctx: any) {body}\nend }}\nreturn module"
                ),
            }],
            None,
        )
        .unwrap()
        .remove(0)
}

pub(super) fn call(contract: &Contract, slot: HookSlot) -> HookCall {
    let views = crate::api::hooks::phase_fields(slot)
        .into_iter()
        .map(|(name, _)| (name, Arc::new(Value::Object(Object::new()))))
        .collect();
    HookCall {
        snapshot: Store::empty(contract.ir().clone()).snapshot(),
        state: contract.state().instantiate(),
        request: None,
        views,
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Cancellation::default(),
        shutdown: Cancellation::default(),
        budget: Default::default(),
        log: Arc::new(|_| {}),
    }
}

fn patched(result: Result<HookOutcome, crate::luau::ScriptError>) -> RequestState {
    match result.unwrap() {
        HookOutcome::Continue(Some(state)) => state,
        _ => panic!("patch required"),
    }
}

#[test]
fn hk_03_04_vm_patch_preserves_omission_null_and_typed_empty_containers_atomically() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::RequestBefore;
    let script = script(
        &contract,
        slot,
        "return {statePatch={authenticated=true,profile={name='Changed'},list={{}},user=ctx.null}}",
    );
    let input = call(&contract, slot);
    let before = input.state.snapshot();
    let after = patched(execute_hook(&script, input)).snapshot();
    assert_eq!(before.get("authenticated"), Some(&Value::Boolean(false)));
    assert_eq!(after.get("authenticated"), Some(&Value::Boolean(true)));
    assert_eq!(after.get("name"), Some(&Value::String("Example".into())));
    assert_eq!(
        after.get("list"),
        Some(&Value::List(vec![Value::Object(Object::new())]))
    );
    assert_eq!(after.get("user"), Some(&Value::Null));
    assert_eq!(
        after.get("profile"),
        Some(&Value::Object(Object::from_iter([
            ("name", Value::String("Changed".into())),
            ("tag", Value::Null)
        ])))
    );
    assert_eq!(contract.state().initial(), before.as_ref());
}

#[test]
fn hk_03_04_vm_rejects_any_return_unknown_keys_bad_shapes_and_partial_nested_replacement() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::OperationBefore;
    for body in [
        "return {}",
        "return true",
        "return ctx.null",
        "return {statePatch={},extra=true}",
        "return {statePatch={unknown=true}}",
        "return {statePatch={profile={}}}",
        "return {statePatch={authenticated=true,count=9007199254740992}}",
        "return {statePatch={authenticated=true,list=ctx.jsonObject({})}}",
        "local t:any={} t[1]=t return {statePatch={list=t}}",
        "return {statePatch={list={[2]={}}}}",
        "return {statePatch={number=0/0}}",
    ] {
        let hook = script(&contract, slot, body);
        let input = call(&contract, slot);
        let before = input.state.snapshot();
        let error = execute_hook(&hook, input).err().unwrap();
        assert!(
            matches!(error.phase, "runtime" | "value"),
            "{body}: {error}"
        );
        assert_eq!(before.as_ref(), contract.state().initial());
    }
    for slot in [
        HookSlot::FieldBefore,
        HookSlot::RequestAfter,
        HookSlot::OperationAfter,
        HookSlot::FieldAfter,
    ] {
        let hook = script(&contract, slot, "return {statePatch={authenticated=true}} ");
        assert!(execute_hook(&hook, call(&contract, slot)).is_err());
    }
}

#[test]
fn hk_03_vm_all_slots_have_readonly_state_models_and_only_declared_phase_members() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for slot in HookSlot::ALL {
        let body = format!(
            "assert(ctx.scope=='read') assert(ctx.state.name=='Example') assert(ctx.models.User:count({{}})==0) assert(ctx.models.User.create==nil) assert(ctx.request{}nil) assert(ctx.reject{}nil) return nil",
            "~=",
            if slot.allows_reject() { "~=" } else { "==" }
        );
        let hook = script(&contract, slot, &body);
        assert!(matches!(
            execute_hook(&hook, call(&contract, slot)).unwrap(),
            HookOutcome::Continue(None)
        ));
        for body in [
            "ctx.state.name='forged'",
            "ctx.state.profile.name='forged'",
            "ctx.models.User:create({name='forged'})",
            "ctx.scope='mutation'",
        ] {
            let hook = script(&contract, slot, body);
            let input = call(&contract, slot);
            let snapshot = input.snapshot.clone();
            assert!(execute_hook(&hook, input).is_err(), "{slot:?}: {body}");
            assert_eq!(snapshot.size().0, 0);
            assert_eq!(
                contract.state().initial().get("name"),
                Some(&Value::String("Example".into()))
            );
        }
    }
}

#[test]
fn hk_10_31_32_vm_first_reject_survives_pcall_second_reject_invalid_values_and_return() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    for slot in [
        HookSlot::RequestBefore,
        HookSlot::OperationBefore,
        HookSlot::FieldBefore,
    ] {
        for (code, status, invalid) in [
            ("401", 401, false),
            ("403", 403, false),
            ("200", 200, false),
            ("204", 500, true),
            ("205", 500, true),
            ("304", 500, true),
            ("600", 500, true),
            ("499", 500, true),
            ("101", 500, true),
            ("401.5", 500, true),
            ("0/0", 500, true),
            ("1/0", 500, true),
            ("'401'", 500, true),
        ] {
            let hook = script(
                &contract,
                slot,
                &format!(
                    "pcall(function() ctx.reject({{code={code},message='Specified'}}) end) pcall(function() ctx.reject({{code=451}}) end) return {{statePatch={{authenticated=true}}}}"
                ),
            );
            let result = execute_hook(&hook, call(&contract, slot));
            // A non-finite codec failure must not be hidden by pcall or a
            // previously recorded invalid rejection.
            if matches!(code, "0/0" | "1/0") {
                assert!(result.is_err(), "{slot:?} code={code}");
                continue;
            }
            let result = result.unwrap_or_else(|error| panic!("{slot:?} code={code}: {error:?}"));
            let HookOutcome::Rejected(rejection, _) = result else {
                panic!("rejection was lost")
            };
            assert_eq!(
                (rejection.status, rejection.invalid),
                (status, invalid),
                "{slot:?}: {code}"
            );
            assert_eq!(
                rejection.message,
                if invalid {
                    "Internal server error"
                } else {
                    "Specified"
                }
            );
        }
    }
}

#[test]
fn sa_02_protected_hook_calls_charge_the_shared_request_work_budget() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::RequestBefore;
    let hook = script(
        &contract,
        slot,
        "for _=1,600 do pcall(function() return 1 end) end return nil",
    );
    let mut input = call(&contract, slot);
    input.budget.0.charge(crate::api::WORK_LIMIT - 512).unwrap();
    let budget = input.budget.clone();
    let error = execute_hook(&hook, input).err().unwrap();
    assert_eq!(error.phase, "runtime");
    assert!(budget.0.check().is_err());
}

#[test]
fn hk_15_vm_logs_are_owned_bounded_and_do_not_override_envelope_fields() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let records = Arc::new(Mutex::new(Vec::<HookLog>::new()));
    for slot in HookSlot::ALL {
        let hook = script(
            &contract,
            slot,
            "ctx.log('info','English\\n\\27[31m',{kind='explicit',value=ctx.null}) return nil",
        );
        let mut input = call(&contract, slot);
        let sink = records.clone();
        input.log = Arc::new(move |record| sink.lock().unwrap().push(record));
        execute_hook(&hook, input).unwrap();
    }
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 6);
    for record in records.iter() {
        assert_eq!(record.level, "info");
        assert_eq!(record.message, "English\n\u{1b}[31m");
        assert_eq!(
            record.fields.get("kind"),
            Some(&Value::String("explicit".into()))
        );
        assert_eq!(record.fields.get("value"), Some(&Value::Null));
    }
    for body in [
        "ctx.log('fatal','bad')",
        "ctx.log('info',true)",
        "ctx.log('info','bad',true)",
        "ctx.log('info','bad',{number=0/0})",
        "local s='x' for i=1,13 do s=s..s end ctx.log('info',s)",
    ] {
        let hook = script(
            &contract,
            HookSlot::RequestBefore,
            &format!("pcall(function() {body} end) return nil"),
        );
        assert!(execute_hook(&hook, call(&contract, HookSlot::RequestBefore)).is_err());
    }
}

#[test]
fn hk_14_vm_uses_remaining_absolute_deadline_and_rejects_foreign_contracts() {
    let _case = crate::vm_gate::test_case();
    let contract = contract();
    let slot = HookSlot::RequestBefore;
    let hook = script(&contract, slot, "while true do end");
    let mut input = call(&contract, slot);
    input.deadline = Instant::now() + Duration::from_millis(25);
    let started = Instant::now();
    assert!(execute_hook(&hook, input).is_err());
    assert!(started.elapsed() < Duration::from_millis(400));
    let hook = script(&contract, slot, "return nil");
    let input = call(&contract, slot);
    input.cancel.cancel();
    assert!(execute_hook(&hook, input).is_err());
    let mut input = call(&contract, slot);
    input.state = StateContract::default().instantiate();
    assert_eq!(execute_hook(&hook, input).err().unwrap().phase, "contract");
    assert!(execute_hook(&hook, call(&contract, slot)).is_ok());
}
