//! Verify JSON string-key type contracts with the real type checker.

#[test]
fn rsm_08_luau_supports_nominal_null_recursive_json_and_quoted_fields() {
    let _test_case = crate::vm_gate::test_case();
    let _lease = crate::vm_gate::test_guard();
    let definitions = "declare extern type ChigiriNull with\nend\n";
    let source = r#"--!strict
    type Json = ChigiriNull | string | number | boolean | {[string]: Json} | {Json}
    type A = { ["end"]: string, ["next"]: B? }
    type B = { ["value"]: Json }
    local function resolve(ctx: {null: ChigiriNull}, value: A): Json
      return { ["field"] = value["end"], ["null"] = ctx.null }
    end
    return resolve
    "#;
    luaur::compile(source).unwrap();
    luaur::check_with_definitions(source, definitions).unwrap();
    let invalid = "--!strict\nlocal function resolve(ctx: {null: ChigiriNull}): string return ctx.null end\nreturn resolve";
    assert!(luaur::check_with_definitions(invalid, definitions).is_err());
}

use super::{Contract, ScriptCandidate, ScriptSet, static_candidates};
use crate::ir::{MockIr, ScriptId};
use chigiri::core::Source;
use std::sync::Arc;

pub(super) fn contract(source: &str) -> Contract {
    Contract::new(Arc::new(
        MockIr::parse(vec![Source::new("types.graphql", source)]).unwrap(),
    ))
    .unwrap()
}
pub(super) fn candidate(contract: &Contract, coordinate: &str, body: &str) -> ScriptCandidate {
    let graph = contract.ir().graph();
    let (parent, field) = coordinate.split_once('.').unwrap();
    let field = graph.types()[graph.type_id(parent).unwrap().0].fields[field];
    let script = contract
        .ir()
        .scripts()
        .iter()
        .position(|plan| plan.field == field)
        .unwrap_or_else(|| panic!("missing script for {coordinate}"));
    ScriptCandidate {
        script: ScriptId(script),
        source: crate::luau::render_module(contract.ir(), ScriptId(script), body).unwrap(),
    }
}

#[test]
fn rsm_combined_project_sources_keep_member_create_contract() {
    let _test_case = crate::vm_gate::test_case();
    let source = format!(
        "{}\n{}",
        include_str!("../../fixtures/projects.graphql"),
        include_str!("../../fixtures/model-api.graphql")
    );
    let combined = contract(&source);
    let base = contract(include_str!("../../fixtures/projects.graphql"));
    base.script(candidate(
        &base,
        "Mutation.addMember",
        "return ctx.models.Member:create({name=args.input.name,status='OPEN',code=args.input.code})",
    ))
    .unwrap();
    combined
        .script(candidate(
            &combined,
            "Mutation.addMember",
            "if ctx.scope == 'mutation' then return ctx.models.Member:create({name=args.input.name,status='OPEN',code=args.input.code}) else error('write requires mutation root') end",
        ))
        .unwrap();
}

#[test]
fn rsm_08_blog_candidates_are_typed_without_domain_branches() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"));
    let candidates = static_candidates(contract.ir()).unwrap();
    assert_eq!(candidates.len(), 6);
    let set = ScriptSet::new(contract, candidates).unwrap();
    assert!(
        set.scripts()
            .iter()
            .all(|script| !script.source().contains(": any")
                && script.source().contains("NOT_IMPLEMENTED"))
    );
}

#[test]
fn rsm_08_read_context_wrong_return_and_root_parent_are_type_errors() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(include_str!("../../fixtures/blog.graphql"));
    for body in [
        "return ctx.models.User:create({name = 'forbidden'})",
        "return 'wrong'",
        "return parent",
        "return ctx.null",
        "return {{id = 'x', name = 42}}",
    ] {
        let result = contract.script(candidate(&contract, "Query.getUsers", body));
        assert!(result.is_err(), "{body}");
    }
    let body = "local row = ctx.models.User:create({name = args.input.name})\nreturn row";
    contract
        .script(candidate(&contract, "Mutation.createUser", body))
        .unwrap();
}

#[test]
fn rsm_18_manual_scaffolds_are_valid_and_error_positions_are_redacted() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { ping: String! }");
    let candidates = static_candidates(contract.ir()).unwrap();
    let set = ScriptSet::new(contract, candidates).unwrap();
    assert!(set.scripts()[0].source().contains("NOT_IMPLEMENTED"));
    let contract = set.contract();
    let bad = candidate(
        contract,
        "Query.ping",
        "local value = 'secret payload'\nreturn 42",
    );
    let error = match contract.script(bad) {
        Err(error) => error,
        Ok(_) => panic!("wrong return accepted"),
    };
    assert_eq!(error.phase, "type");
    assert_eq!(error.body.0, 5);
    assert!(!error.to_string().contains("secret"));
    let syntax = candidate(contract, "Query.ping", "local value = 1\nreturn (");
    assert_eq!(contract.script(syntax).err().unwrap().phase, "compile");
}

#[test]
fn rsm_08_script_body_cannot_escape_or_shadow_the_expected_wrapper() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { text: String! }");
    for body in [
        "return 'ok'\nend\nlocal function resolve() return 42 end\ndo",
        "return 'ok'\nend\nlocal function replacement() return 42 end\nlocal resolve = replacement\nlocal function unused()",
        "return 'ok'\nend --[[",
    ] {
        let error = contract
            .script(candidate(&contract, "Query.text", body))
            .err()
            .unwrap();
        assert_eq!(error.phase, "compile");
    }
    // A valid body containing a function, branch, long string, and comment remains accepted.
    let valid = "--[=[ end ]=]\nlocal function text() return [=[end]=] end\nif true then return text() else return 'x' end";
    contract
        .script(candidate(&contract, "Query.text", valid))
        .unwrap();
}

#[test]
fn af_09_runtime_message_text_never_establishes_a_source_position() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract("type Query { text: String! }");
    let script = contract
        .script(candidate(&contract, "Query.text", "return 'ok'"))
        .unwrap();
    let error = script.runtime_error("runtime");
    assert_eq!(error.source_position(), None);
}
