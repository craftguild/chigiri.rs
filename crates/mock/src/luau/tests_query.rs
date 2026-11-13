//! Verify Luau and Rust agree on field types, operators, sort directions, and read permissions.

use super::tests::{candidate, contract};
use crate::ir::MockIr;
use chigiri::core::Source;
use std::sync::Arc;

#[test]
fn mr_01_03_14_query_signatures_reject_wrong_types_operators_and_write_capabilities() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        enum Status { OPEN CLOSED }
        type Item { id:ID! name:String! rank:Int! enabled:Boolean! status:Status! }
        type Query { items:[Item!]! total:Int! }
    "#,
    );
    for body in [
        "return ctx.models.Item:find()",
        "return ctx.models.Item:find({where = {field = 'missing', op = 'eq', value = 1}})",
        "return ctx.models.Item:find({where = {field = 'rank', op = 'eq', value = '1'}})",
        "return ctx.models.Item:find({where = {field = 'enabled', op = 'lt', value = true}})",
        "return ctx.models.Item:find({where = {field = 'id', op = 'contains', value = 'x'}})",
        "return ctx.models.Item:find({where = {field = 'name', op = 'eq', value = ctx.null}})",
        "return ctx.models.Item:find({where = {field = 'status', op = 'eq', value = 'UNKNOWN'}})",
        "return ctx.models.Item:find({orderBy = {{field = 'missing', direction = 'asc'}}})",
        "return ctx.models.Item:find({offset = ctx.null})",
        "return ctx.models.Item:find({limit = ctx.null})",
        "ctx.models.Item:create({name = 'x', rank = 1, enabled = true}) return {}",
        "ctx.models.Item:update('x', {name = 'x'}) return {}",
        "ctx.models.Item:delete('x') return {}",
    ] {
        assert!(
            contract
                .script(candidate(&contract, "Query.items", body))
                .is_err(),
            "{body}"
        );
    }
    for body in [
        "return ctx.models.Item:count()",
        "return ctx.models.Item:count({where = {field = 'rank', op = 'eq', value = 'x'}})",
    ] {
        assert!(
            contract
                .script(candidate(&contract, "Query.total", body))
                .is_err(),
            "{body}"
        );
    }
    contract
        .script(candidate(
            &contract,
            "Query.total",
            "return ctx.models.Item:count({})",
        ))
        .unwrap();
    contract
        .script(candidate(
            &contract,
            "Query.items",
            "return ctx.models.Item:find({orderBy = {{field = 'name', direction = 'asc'}}})",
        ))
        .unwrap();
    contract
        .script(candidate(
            &contract,
            "Query.items",
            "return ctx.models.Item:find({where = {field = 'status', op = 'eq', value = 'OPEN'}})",
        ))
        .unwrap();
    contract
        .script(candidate(
            &contract,
            "Query.items",
            "local model = ctx.models.Item\nreturn model:find({where={field='status',op='eq',value='OPEN'}})",
        ))
        .unwrap();
    contract
        .script(candidate(
            &contract,
            "Query.items",
            "return ctx.models.Item:find({where={field='status',op='eq',value=(function(): ChigiriStatusValue local unrelated='NOT_A_STATUS'; assert(unrelated == 'NOT_A_STATUS'); return 'OPEN' end)()}})",
        ))
        .unwrap();
    contract.script(candidate(&contract, "Query.items", "local order: {{field: 'name' | 'rank', direction: 'asc' | 'desc'}} = {{field = 'name', direction = 'asc'}, {field = 'rank', direction = 'desc'}}\nreturn ctx.models.Item:find({orderBy = order})")).unwrap();
}

#[test]
fn sa_03_inline_enum_predicates_survive_same_named_fields_across_models() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        enum FirstStatus { OPEN CLOSED }
        enum SecondStatus { ACTIVE INACTIVE }
        scalar Data @representation(type: JSON)
        type First { id:ID! status:FirstStatus! data:Data! }
        type Second { id:ID! status:SecondStatus! }
        type Plain { id:ID! status:String! }
        type Query { first:[First!]! second:[Second!]! plain:[Plain!]! }
        type Mutation { createFirst:Boolean! }
        "#,
    );
    for (coordinate, model, value) in [
        ("Query.first", "First", "OPEN"),
        ("Query.second", "Second", "ACTIVE"),
        ("Query.plain", "Plain", "free-form"),
    ] {
        let body = format!(
            "return ctx.models.{model}:find({{where={{field='status',op='eq',value='{value}'}}}})"
        );
        contract
            .script(candidate(&contract, coordinate, &body))
            .unwrap();
    }
    contract
        .script(candidate(
            &contract,
            "Mutation.createFirst",
            "ctx.models.First:create({id='created',status='OPEN',data=ctx.jsonObject({field='status',value='free-form'})}) return true",
        ))
        .unwrap();
}

#[test]
fn ab_01_02_inline_query_context_covers_ordering_indexing_and_bracket_keys() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        enum Status { OPEN CLOSED }
        type Item { id:ID! rank:Int! status:Status! }
        type Query { items:[Item!]! }
        "#,
    );
    for body in [
        r#"return ctx.models.Item:find({
            where={all={
                {field='status',op='eq',value='OPEN'},
                {field='rank',op='gte',value=0},
            }},
            orderBy={
                {field='rank',direction='desc'},
                {field='id',direction='asc'},
            },
        })"#,
        "local first=ctx.models.Item:find({where={field='status',op='eq',value='OPEN'}})[1]; return ctx.models.Item:find({})",
        "local first=(ctx.models.Item:find({where={field='status',op='eq',value='OPEN'}}))[1]; return ctx.models.Item:find({})",
        "local first=ctx.models['Item']:find({['where']={['field']='status',['op']='eq',['value']='OPEN'}})[1]; return ctx.models.Item:find({})",
        "local model=ctx.models.Item; local first=model:find({where={field='status',op='eq',value='OPEN'}})[1]; return model:find({})",
    ] {
        let candidate = candidate(&contract, "Query.items", body);
        let narrowed = super::literal_precision::narrow(&candidate.source, contract.ir()).0;
        contract.script(candidate).unwrap_or_else(|error| {
            panic!("valid contextual query failed: {body}\n{error:?}\n{narrowed}")
        });
    }
}

#[test]
fn ab_03_private_record_slots_are_composed_once_for_numeric_operations() {
    let _test_case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        "type Account{id:ID!} type Query{nextBalance:Float!}",
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "extend type Account{balance:Float!}",
    )];
    let contract = super::Contract::new(Arc::new(
        MockIr::parse_with_storage(public, storage).unwrap(),
    ))
    .unwrap();
    contract
        .script(candidate(
            &contract,
            "Query.nextBalance",
            "local account=ctx.models.Account:find({limit=1})[1]; return account.balance + 1",
        ))
        .unwrap();
    let declarations = contract.declarations();
    let record = declarations
        .split("type ChigiriAccountRecord = ChigiriAccountStored & {")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .expect("account record declaration");
    assert!(!record.contains("balance"), "{record}");
}
