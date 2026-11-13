//! Verify each module retains an independent scope and diagnostic position within batched checking.

use super::ScriptSet;
use super::tests::{candidate, contract};

#[test]
fn sa_01_checker_bindings_are_derived_from_the_capability_registry() {
    let prelude = super::checker_prelude();
    for function in crate::api::runtime::GLOBALS {
        let binding = format!("local {}: {}", function.name, function.function_type());
        assert_eq!(prelude.contains(&binding), !function.checker_intrinsic());
    }
    for library in crate::api::runtime::LIBRARIES {
        assert!(prelude.contains(&format!("local {}: {}", library.name, library.type_name)));
    }
}

#[test]
fn batch_keeps_exported_types_inside_each_module() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { first: Int! second: Int! }");
    let mut first = candidate(&contract, "Query.first", "return 1");
    first
        .source
        .insert_str(0, "export type LocalOnly = number\n");
    let mut second = candidate(&contract, "Query.second", "return 2");
    second.source.insert_str(0, "local leaked: LocalOnly = 1\n");
    let error = ScriptSet::new(contract, vec![first, second]).err().unwrap();
    assert_eq!(error.script, Some(crate::ir::ScriptId(1)));
    assert_eq!(error.phase, "type");
}

#[test]
fn batch_allows_same_local_type_name_in_separate_modules() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { first: Int! second: String! }");
    let mut first = candidate(&contract, "Query.first", "return 1");
    first.source.insert_str(0, "export type Local = number\n");
    let mut second = candidate(&contract, "Query.second", "return 'ok'");
    second.source.insert_str(0, "export type Local = string\n");
    ScriptSet::new(contract, vec![first, second]).unwrap();
}

#[test]
fn batch_checks_numeric_comparison_with_the_upstream_one_shot_checker() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! } ");
    let source = "local function dive(n: number): ()\n    if n == 0 then return end\n    pcall(dive, n - 1)\nend\ndive(17)\nreturn 42";
    let candidate = candidate(&contract, "Query.value", source);
    ScriptSet::new(contract, vec![candidate]).unwrap();
}

#[test]
fn later_resolver_type_failure_keeps_its_coordinate_and_source_line() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { first: Int! second: Int! }");
    let candidates = vec![
        candidate(&contract, "Query.first", "return 1"),
        candidate(&contract, "Query.second", "local value = 1\nreturn 'wrong'"),
    ];
    let line = candidates[1]
        .source
        .lines()
        .position(|line| line.contains("return 'wrong'"))
        .unwrap()
        + 1;
    let error = ScriptSet::new(contract, candidates).err().unwrap();
    assert_eq!(error.script, Some(crate::ir::ScriptId(1)));
    assert_eq!(error.phase, "type");
    assert_eq!(error.cause, "MOCK_TYPE_MISMATCH");
    assert_eq!(error.body.0, line);
    assert_eq!(error.member.as_deref(), Some("Query.second"), "{error:?}");
    assert_eq!(error.expected, "Int! result for Query.second");
    assert_eq!(error.actual, "string");
    assert_eq!(
        error.hint,
        "Return a value compatible with the resolver result shown in expected."
    );
}

#[test]
fn sa_16_diagnostic_actual_does_not_guess_from_a_compound_expression_prefix() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let candidate = candidate(&contract, "Query.value", "return 1 == 2");
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.cause, "MOCK_CONTRACT");
    assert_eq!(error.actual, "structural type unavailable");
    assert_eq!(
        error.hint,
        "Review the generated declarations and reported logical position."
    );
}

#[test]
fn sa_16_diagnostic_uses_the_assignment_not_an_unrelated_return() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let candidate = candidate(
        &contract,
        "Query.value",
        "local wrong: number = 'text'; return false",
    );
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.cause, "MOCK_TYPE_MISMATCH");
    assert_eq!(error.member.as_deref(), Some("local binding"));
    assert_eq!(error.expected, "number");
    assert_eq!(error.actual, "string");
}

#[test]
fn sa_12_invalid_enum_predicate_reports_the_schema_member_and_enum_shape() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    let candidate = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({where={field='status',op='eq',value='UNKNOWN'}})",
    );
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("Item.status"));
    assert!(error.expected.contains("enum Status for Item.status"));
    assert!(error.expected.contains("OPEN | CLOSED"));
    assert_eq!(error.actual, "string outside declared enum");
    assert_eq!(
        error.hint,
        "Use one of the enum alternatives shown in expected."
    );
}

#[test]
fn sa_12_enum_list_reports_a_non_string_item() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    let candidate = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({where={field='status',op='in',value={'OPEN',1}}})",
    );
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("Item.status"));
    assert!(error.expected.contains("enum Status for Item.status"));
    assert!(error.expected.contains("OPEN | CLOSED"));
    assert_eq!(error.actual, "number");
    assert_eq!(
        error.hint,
        "Use one of the enum alternatives shown in expected."
    );
}

#[test]
fn sa_12_long_bracket_enum_literals_keep_literal_precision() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    for body in [
        "return ctx.models.Item:find({where={field='status',op='eq',value=[[OPEN]]}})",
        "return ctx.models.Item:find({where={field='status',op='in',value={[[OPEN]],[[CLOSED]]}}})",
    ] {
        contract
            .script(candidate(&contract, "Query.items", body))
            .unwrap();
    }
}

#[test]
fn sa_12_non_enum_predicate_reports_the_field_type_and_member() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID! rank:Int!} type Query{items:[Item!]!}");
    let candidate = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({where={field='rank',op='eq',value='high'}})",
    );
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("Item.rank"), "{error:?}");
    assert_eq!(error.expected, "Int value for Item.rank");
    assert_eq!(error.actual, "string");
    assert_eq!(
        error.hint,
        "Use a value compatible with the field type shown in expected."
    );
}

#[test]
fn sa_12_invalid_order_direction_reports_the_order_member() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID! name:String!} type Query{items:[Item!]!}");
    let script = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({orderBy={{field='name',direction='sideways'}}})",
    );
    let error = contract.script(script).err().unwrap();
    assert_eq!(
        error.member.as_deref(),
        Some("Item.orderBy.direction"),
        "{error:?}"
    );
    assert_eq!(
        error.expected,
        "sort direction for Item.orderBy.direction (asc | desc)"
    );
    assert_eq!(error.actual, "string outside sort direction");
    assert_eq!(error.hint, "Use asc or desc.");

    let script = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({orderBy={{field='name',direction=1}}})",
    );
    let error = contract.script(script).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("Item.orderBy.direction"));
    assert_eq!(error.actual, "number");
}

#[test]
fn sa_12_valid_order_site_does_not_mask_an_invalid_limit() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID! rank:Int!} type Query{items:[Item!]!}");
    let script = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({orderBy={{field='rank',direction='asc'}},limit='bad'})",
    );
    let error = contract.script(script).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("Item.find.limit"));
    assert_eq!(error.expected, "nonnegative integer for Item.find.limit");
    assert_eq!(error.actual, "string");
    assert_eq!(
        error.hint,
        "Use a nonnegative integer within the supported range."
    );
}

#[test]
fn sa_12_predicate_diagnostic_uses_the_operator_operand_type() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID! rank:Int!} type Query{items:[Item!]!}");
    for (operator, expected) in [
        ("in", "list of Int values for Item.rank"),
        ("isNull", "boolean isNull operand for Item.rank"),
    ] {
        let body = format!(
            "return ctx.models.Item:find({{where={{field='rank',op='{operator}',value=1}}}})"
        );
        let script = candidate(&contract, "Query.items", &body);
        let error = contract.script(script).err().unwrap();
        assert_eq!(error.member.as_deref(), Some("Item.rank"));
        assert_eq!(error.expected, expected);
        assert_eq!(error.actual, "number");
    }
}

#[test]
fn sa_12_ineligible_operator_does_not_claim_a_value_mismatch() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID! rank:Int!} type Query{items:[Item!]!}");
    let script = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({where={field='rank',op='contains',value='x'}})",
    );
    let error = contract.script(script).err().unwrap();
    assert_eq!(error.member, None);
    assert_eq!(
        error.expected,
        "closed resolver module { resolve: generated function }"
    );
}

#[test]
fn sa_12_enum_expression_does_not_claim_an_inner_assignment_error() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    let candidate = candidate(
        &contract,
        "Query.items",
        "return ctx.models.Item:find({where={field='status',op='eq',value=(function(): ChigiriStatusValue local value:number='text'; return 'OPEN' end)()}})",
    );
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("local binding"), "{error:?}");
    assert_eq!(error.expected, "number");
    assert_eq!(error.actual, "string");
    assert_eq!(
        error.hint,
        "Assign a value compatible with the declared type."
    );
}

#[test]
fn sa_12_unicode_prefix_keeps_enum_diagnostic_site_and_byte_column() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! status:Status!} type Query{items:[Item!]!}",
    );
    let body = "local note='résumé naïve façade élève déjà'; return ctx.models.Item:find({where={field='status',op='eq',value='UNKNOWN'}})";
    let candidate = candidate(&contract, "Query.items", body);
    let error = ScriptSet::new(contract, vec![candidate]).err().unwrap();
    assert_eq!(error.member.as_deref(), Some("Item.status"));
    assert!(error.expected.contains("enum Status for Item.status"));
    assert_eq!(error.actual, "string outside declared enum");
    assert!(error.body.1 > body.find("'UNKNOWN'").unwrap());
}

#[test]
fn sa_01_checker_uses_the_closed_executable_capability_contract() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    let supported = r#"
local values = {3, 1}
table.insert(values, 1, 4)
table.insert(values, 2)
table.sort(values)
assert(table.concat(values, ",") == "1,2,3,4")
assert(string.len("abc") == 3 and ("ABC"):lower() == "abc")
assert(string.sub("abc", 2) == "bc")
assert(string.find("abc", "b", 1, true) == 2)
assert(math.abs(-2) == 2 and math.ceil(1.2) == 2 and math.floor(1.8) == 1)
assert(tostring(2) == "2" and tonumber("2") == 2)
assert(os.time({year=1970,month=1,day=1,hour=0,min=0,sec=0}) == 0)
assert(os.date("!%Y-%m-%d", 0) == "1970-01-01")
assert(os.difftime(7, 2) == 5 and type(os.clock()) == "number")
assert(type(_G.os.clock()) == "number")
return math.min(values[1], math.max(values[2], values[3]))
"#;
    contract
        .script(candidate(&contract, "Query.value", supported))
        .unwrap();
    for body in [
        "return string.rep('x', 2)",
        "return ('x'):rep(2)",
        "return math.sqrt(4)",
        "return table.freeze({}) and 1",
        "return os.execute('true') and 1",
        "return _G.os.getenv('HOME') and 1",
    ] {
        assert!(
            contract
                .script(candidate(&contract, "Query.value", body))
                .is_err(),
            "{body}"
        );
    }
    contract
        .script(candidate(
            &contract,
            "Query.value",
            "local os = {clock = function() return 7 end} return os.clock()",
        ))
        .unwrap();
}

#[test]
fn tl_14_resolvers_reject_removed_clock_and_timeleap_control() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Query { value: Int! }");
    for body in [
        "return ctx.clock.now()",
        "timeleap.travel_to(1); return 1",
        "_G.timeleap.reset(); return 1",
    ] {
        let error = contract
            .script(candidate(&contract, "Query.value", body))
            .err()
            .unwrap();
        assert_eq!(error.phase, "type", "{body}: {error:?}");
    }
}
