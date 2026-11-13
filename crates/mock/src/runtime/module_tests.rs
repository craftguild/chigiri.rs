//! Validate initialization and request execution from the same source in fresh VM globals.

use chigiri::core::{Object, Value};

use super::tests::{contract, run};
use crate::ir::ScriptId;
use crate::luau::ScriptCandidate;
use crate::output::OutputValue;
use crate::store::Store;

#[test]
fn mp_06_20_22_initialization_has_no_context_does_not_resolve_and_retains_no_closure() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let module = |source| ScriptCandidate {
        script: ScriptId(0),
        source,
    };
    let source = r#"
local globals = _G :: any
assert(globals.ctx == nil)
assert(globals.persisted == nil)
globals.persisted = true
local calls = 0
return {resolve=function(): number
    calls += 1
    return calls
end}
"#;
    let script = contract.script(module(source.to_owned())).unwrap();
    let mut store = Store::empty(contract.ir().clone());
    for _ in 0..4 {
        assert!(matches!(
            run(&script, &Object::new(), &mut store).unwrap(),
            OutputValue::Scalar(Value::Int(1))
        ));
    }
    let never_called = "return {resolve=function(): number error('private resolve') end}";
    let script = contract.script(module(never_called.into())).unwrap();
    let error = run(&script, &Object::new(), &mut store).err().unwrap();
    assert_eq!(error.phase, "runtime");
    assert!(!error.to_string().contains("private"));
    let changed_owned_source = String::from("return {resolve=function(): number return 42 end}");
    let script = contract
        .script(module(changed_owned_source.clone()))
        .unwrap();
    drop(changed_owned_source);
    assert!(matches!(
        run(&script, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::Int(42))
    ));
}
