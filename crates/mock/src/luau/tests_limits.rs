//! Check declaration, body, and wrapper capacities before invoking the compiler.

use std::sync::Arc;

use chigiri::core::Source;

use super::tests::{candidate, contract};
use super::{Contract, MAX_BODY, MAX_GENERATED, MAX_WRAPPER, ScriptSet, static_candidates};
use crate::ir::MockIr;

#[test]
fn rsm_16_luau_body_and_renderer_byte_limits_are_inclusive() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: String! }");
    let mut source = candidate(&contract, "Query.value", "return 'ok'").source;
    source.push_str(&" ".repeat(MAX_BODY - source.len()));
    let module = |source: String| super::ScriptCandidate {
        script: crate::ir::ScriptId(0),
        source,
    };
    contract.script(module(source.clone())).unwrap();
    source.push(' ');
    assert_eq!(
        contract.script(module(source)).err().unwrap().phase,
        "limit"
    );
    let mut out = super::text::Text::new(MAX_WRAPPER);
    out.add(&"x".repeat(MAX_WRAPPER)).unwrap();
    assert!(out.add("x").is_err());
    assert_eq!(out.finish().len(), MAX_WRAPPER);
}

#[test]
fn rsm_16_oneof_declaration_expansion_is_bounded_before_typechecking() {
    let _test_case = crate::vm_gate::test_case();
    let fields = (0..512)
        .map(|id| format!("field{id}: String\n"))
        .collect::<String>();
    let source =
        format!("input Pick @oneOf {{ {fields} }} type Query {{ value(pick: Pick): String! }}");
    let ir = Arc::new(MockIr::parse(vec![Source::new("wide.graphql", source)]).unwrap());
    assert_eq!(Contract::new(ir).err().unwrap().phase, "limit");
}

#[test]
fn rsm_18_script_coverage_rejects_missing_duplicate_and_unknown_coordinates() {
    let _test_case = crate::vm_gate::test_case();
    let contract = || contract("type Query { a: String! b: String! }");
    for variant in 0..3 {
        let contract = contract();
        let mut candidates = static_candidates(contract.ir()).unwrap();
        match variant {
            0 => {
                candidates.pop();
            }
            1 => {
                candidates[1].script = candidates[0].script;
            }
            _ => {
                candidates[1].script = crate::ir::ScriptId(999);
            }
        }
        assert!(ScriptSet::new(contract, candidates).is_err());
    }
    let contract = contract();
    let mut candidates = static_candidates(contract.ir()).unwrap();
    candidates[1].source = " ".repeat(MAX_BODY + 1);
    let error = ScriptSet::new(contract, candidates).err().unwrap();
    assert_eq!(error.phase, "limit");
    assert_eq!(error.script, Some(crate::ir::ScriptId(1)));
}

#[test]
fn rsm_16_all_wrapper_bytes_are_bounded_before_compiler_invocation() {
    let _test_case = crate::vm_gate::test_case();
    // Each generated wrapper stays below 1 MiB while their combined size exceeds 16 MiB.
    let fields = (0..300)
        .map(|id| format!("f{id}{}: String!\n", "x".repeat(180)))
        .collect::<String>();
    let roots = (0..64)
        .map(|id| format!("r{id}: String!\n"))
        .collect::<String>();
    let source = format!("type Item {{ id: ID! {fields} }} type Query {{ {roots} }}");
    let contract = contract(&source);
    assert!(contract.declarations().len() < MAX_WRAPPER);
    assert!(contract.declarations().len() * 64 > MAX_GENERATED);
    let mut candidates = static_candidates(contract.ir()).unwrap();
    // Capacity rejection must precede any syntax error in the oversized input.
    candidates[0].source = "return (".into();
    assert_eq!(
        ScriptSet::new(contract, candidates).err().unwrap().phase,
        "limit"
    );
}
