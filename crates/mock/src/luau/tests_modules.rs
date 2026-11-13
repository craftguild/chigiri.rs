//! Verify module syntax, expected resolver types, and Rust-side closed-shape checks after casts.

use super::tests::contract;
use super::{ScriptCandidate, ScriptSet, static_candidates};
use crate::ir::ScriptId;

fn module(source: &str) -> ScriptCandidate {
    ScriptCandidate {
        script: ScriptId(0),
        source: source.to_owned(),
    }
}

#[test]
fn cv_02_module_closed_shape_and_function_are_checked_after_casts() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    for value in [
        "nil",
        "42",
        "function() end",
        "{}",
        "{state='ready'}",
        "{state='Ready',resolve=function() return 1 end}",
        "{state='ready',resolve={}}",
        "{state='ready',resolve=function() return 1 end,extra=true}",
        "{[1]='ready',resolve=function() return 1 end}",
    ] {
        let source = format!("local value: any = {value}\nreturn value");
        assert_eq!(
            contract.script(module(&source)).err().unwrap().phase,
            "module",
            "{value}"
        );
    }
    let valid = "return {resolve=function(): number return 1 end}";
    let script = contract.script(module(valid)).unwrap();
    assert_eq!(script.source(), valid);
    let legacy = "return {state='ready',resolve=function(): number return 1 end}";
    let error = match contract.script(module(legacy)) {
        Ok(_) => panic!("legacy state descriptor accepted"),
        Err(error) => error,
    };
    assert!(matches!(error.phase, "type" | "module"));
    let set = ScriptSet::new(contract, vec![module(valid)]).unwrap();
    assert_eq!(set.scripts().len(), 1);
}

#[test]
fn mp_03_21_modules_are_independently_typed_and_cannot_escape_expected_return() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value(count: Int!): Int! }");
    for source in [
        "return {resolve=function(): string return 'wrong' end}",
        "return {resolve=function(ctx: ChigiriWriteContext, args: ChigiriQueryValueArgs): number return 1 end}",
        "return {resolve=function(ctx: ChigiriReadContext, args: {count:string}): number return 1 end}",
        "local bad: string = 42\nreturn {resolve=function(): number return 1 end}",
    ] {
        assert_eq!(
            contract.script(module(source)).err().unwrap().phase,
            "type",
            "{source}"
        );
    }
    for source in [
        "return {}\nend\nlocal function module(): any return {} end\ndo",
        "return {}\nend --[[",
    ] {
        assert_eq!(
            contract.script(module(source)).err().unwrap().phase,
            "compile"
        );
    }
    let candidates = static_candidates(contract.ir()).unwrap();
    assert!(candidates[0].source.contains("ChigiriQueryValueResolver"));
    assert!(candidates[0].source.contains("ChigiriQueryValueContext"));
    assert!(candidates[0].source.contains("ChigiriQueryValueResult"));
    assert!(!candidates[0].source.contains("type Chigiri"));
}
