//! Exercises the selected native time surface through real resolver VMs.

use std::time::Instant;

use chigiri::core::{Object, Value};

use super::tests::{contract, run, script};
use crate::ir::ScriptId;
use crate::luau::ScriptCandidate;
use crate::output::OutputValue;
use crate::store::Store;

#[test]
fn tl_01_os_exposes_exactly_four_readonly_members() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { probe: Boolean! }");
    let inspect = script(
        &contract,
        "Query.probe",
        r#"
local expected = {time=true, date=true, difftime=true, clock=true}
local count = 0
for name, member in pairs(os) do
    assert(expected[name] == true and type(member) == "function")
    count += 1
end
local dynamic: any = os
return count == 4 and dynamic.execute == nil and dynamic.getenv == nil
"#,
    );
    let mut store = Store::empty(contract.ir().clone());
    assert!(matches!(
        run(&inspect, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));

    let mutate = script(
        &contract,
        "Query.probe",
        "local dynamic: any = os; dynamic.time = function() return 1 end; return true",
    );
    assert!(run(&mutate, &Object::new(), &mut store).is_err());
}

#[test]
fn tl_03_native_explicit_overloads_defaults_and_utc_behavior_are_preserved() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { probe: Boolean! }");
    let resolver = script(
        &contract,
        "Query.probe",
        r#"
local epoch = os.time({year=1970, month=1, day=1, hour=0, min=0, sec=0})
local noon = os.time({year=1970, month=1, day=1})
local nilTime = os.time(nil)
local fields = os.date("!*t", 0)
assert(fields ~= nil)
local dynamic: any = os
local nativeOk = pcall(function() os.date("%L", 0) end)
return epoch == 0
    and noon == 43200
    and os.difftime(10) == 10
    and os.difftime(10, 3) == 7
    and os.date("!%Y-%m-%dT%H:%M:%S", 0) == "1970-01-01T00:00:00"
    and dynamic.date("!%Y", "0") == "1970"
    and os.date("", -1) == nil
    and os.date("!%Y", -1) == "1969"
    and os.time({year=1969, month=12, day=31, hour=23, min=59, sec=59}) == nil
    and type(nilTime) == "number"
    and nativeOk == false
    and fields.year == 1970 and fields.month == 1 and fields.day == 1
    and fields.hour == 0 and fields.min == 0 and fields.sec == 0
"#,
    );
    let mut store = Store::empty(contract.ir().clone());
    assert!(matches!(
        run(&resolver, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));

    let source = crate::luau::render_module(
        contract.ir(),
        ScriptId(0),
        r#"
local format: string = "!*t"
local value = os.date(format, 0)
return value ~= nil and string.len(value) > 0
"#,
    )
    .unwrap();
    let error = contract
        .script(ScriptCandidate {
            script: ScriptId(0),
            source,
        })
        .err()
        .expect("dynamic date formats must retain the table return possibility");
    assert_eq!(error.phase, "type");
}

#[test]
fn tl_01_dynamic_date_format_keeps_every_native_return_shape() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { probe: Boolean! }");
    let resolver = script(
        &contract,
        "Query.probe",
        r#"
local format: string = "!*t"
local value = os.date(format, 0)
local defaultValue = os.date()
local nilFormat = os.date(nil, 0)
assert(defaultValue == nil or string.len(defaultValue) > 0)
assert(nilFormat == nil or string.len(nilFormat) > 0)
if type(value) == "table" then
    return value.year == 1970 and value.month == 1 and value.day == 1
elseif type(value) == "string" then
    return string.len(value) > 0
end
return false
"#,
    );
    let mut store = Store::empty(contract.ir().clone());
    assert!(matches!(
        run(&resolver, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}

#[test]
fn tl_01_native_argument_errors_remain_catchable() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { probe: Boolean! }");
    let bodies = [
        r#"
local input: any = {year={}, month=1, day=1}
local ok = pcall(function() os.time(input) end)
return not ok and os.time({year=1970, month=1, day=1, hour=0}) == 0
"#,
        r#"
local timestamp: any = {}
local ok = pcall(function() os.date("!%Y", timestamp) end)
return not ok and os.date("!%Y", 0) == "1970"
"#,
    ];
    let mut store = Store::empty(contract.ir().clone());
    for body in bodies {
        let resolver = script(&contract, "Query.probe", body);
        assert!(matches!(
            run(&resolver, &Object::new(), &mut store).unwrap(),
            OutputValue::Scalar(Value::Boolean(true))
        ));
    }
}

#[test]
fn tl_02_apparent_current_time_substitution_survives_module_aliases() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { probe: Boolean! }");
    let source = r#"
local readCurrent = os.time
local formatCurrent = os.date
return {resolve=function(): boolean
    return readCurrent() == 0
        and formatCurrent("!%Y-%m-%dT%H:%M:%S") == "1970-01-01T00:00:00"
        and formatCurrent("!%Y-%m-%dT%H:%M:%S", nil) == "1970-01-01T00:00:00"
        and formatCurrent() == formatCurrent(nil, 0)
        and os.date("!%Y", 1) == "1970"
end}
"#;
    let candidate = ScriptCandidate {
        script: ScriptId(0),
        source: source.to_owned(),
    };
    let resolver = contract.script(candidate).unwrap();
    let mut store = Store::empty(contract.ir().clone());
    let server = crate::clock::ServerClock::new(&crate::clock::ClockSettings::fixed(0).unwrap());
    let budget = super::ExecutionBudget::default();
    budget.set_clock(&server);
    let execution = super::Execution {
        state: resolver.initial_state(),
        request: None,
        deadline: Instant::now() + super::VM_TIME,
        cancel: Default::default(),
        shutdown: Default::default(),
        budget,
    };
    let snapshot = store.snapshot();
    assert!(matches!(
        super::execute_root_with(&resolver, &Object::new(), &mut store, &snapshot, &execution,)
            .unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));
}

#[test]
fn tl_07_initialization_allows_explicit_conversion_but_rejects_live_time() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { probe: Boolean! }");
    let explicit = r#"
local epoch = os.time({year=1970, month=1, day=1, hour=0, min=0, sec=0})
local rendered = os.date("!%Y-%m-%d", epoch)
local elapsed = os.difftime((epoch :: number) + 5, epoch)
return {resolve=function(): boolean
    return epoch == 0 and rendered == "1970-01-01" and elapsed == 5
end}
"#;
    let module = |source: &str| ScriptCandidate {
        script: ScriptId(0),
        source: source.to_owned(),
    };
    let resolver = contract.script(module(explicit)).unwrap();
    let mut store = Store::empty(contract.ir().clone());
    assert!(matches!(
        run(&resolver, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Boolean(true))
    ));

    for live in [
        "local value = os.time()",
        "local value = os.date()",
        "local value = os.clock()",
    ] {
        let source =
            format!("{live}\nreturn {{resolve=function(): boolean return value ~= nil end}}");
        let error = contract.script(module(&source)).err().unwrap();
        assert!(
            matches!(error.phase, "initialization" | "limit" | "runtime"),
            "{live}: {error:?}"
        );
    }
}
