//! ADR-0047 generated-record and query-context acceptance.

use std::sync::Arc;

use chigiri::core::Source;

use super::tests::candidate;
use super::{Contract, ScriptCandidate};
use crate::ir::MockIr;

fn contract(public: &str, storage: &str) -> Contract {
    let storage = if storage.is_empty() {
        Vec::new()
    } else {
        vec![Source::new("storage.graphql", storage)]
    };
    Contract::new(Arc::new(
        MockIr::parse_with_storage(vec![Source::new("schema.graphql", public)], storage).unwrap(),
    ))
    .unwrap()
}

fn check(contract: &Contract, coordinate: &str, body: &str) {
    contract
        .script(candidate(contract, coordinate, body))
        .unwrap_or_else(|error| panic!("valid authoring form failed: {body}\n{error:?}"));
}

fn reject(contract: &Contract, coordinate: &str, body: &str) {
    assert!(
        contract
            .script(candidate(contract, coordinate, body))
            .is_err(),
        "invalid authoring form passed: {body}"
    );
}

#[test]
fn ab_01_query_context_covers_every_query_accepting_method() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        r#"
        enum Status { OPEN CLOSED }
        type Owner { id:ID! rank:Int! status:Status! end:[Target!]! @relation }
        type Target { id:ID! rank:Int! status:Status! owners:[Owner!]! @relation }
        type Query { probe:Boolean! }
        type Mutation { probe:Boolean! }
        "#,
        "",
    );
    let where_ =
        "where={all={{field='status',op='eq',value='OPEN'},{field='rank',op='gte',value=0}}}";
    let order = "orderBy={{field='rank',direction='desc'},{field='id',direction='asc'}}";
    check(
        &contract,
        "Query.probe",
        &format!("return ctx.models.Owner:count({{{where_}}}) >= 0"),
    );
    check(
        &contract,
        "Query.probe",
        &format!("return #ctx.models.Owner:find({{{where_},{order}}}) >= 0"),
    );
    for method in ["update", "delete"] {
        let tail = if method == "update" { ", {rank=2}" } else { "" };
        check(
            &contract,
            "Mutation.probe",
            &format!("ctx.models.Owner:{method}({{{where_},{order}}}{tail}); return true"),
        );
    }
    for method in ["connect", "disconnect"] {
        check(
            &contract,
            "Mutation.probe",
            &format!(
                "ctx.models.Owner:{method}({{{where_},{order}}},'end',{{where={{field='status',op='eq',value='OPEN'}},{order}}}); return true"
            ),
        );
    }
    let update = contract
        .script(candidate(
            &contract,
            "Mutation.probe",
            "ctx.models.Owner:update({orderBy={{field='rank',direction='asc'}},limit='wide'},{rank=2}); return true",
        ))
        .err()
        .unwrap();
    assert_eq!(update.member.as_deref(), Some("Owner.update.limit"));
    assert_eq!(update.actual, "string");
    let target = contract
        .script(candidate(
            &contract,
            "Mutation.probe",
            "ctx.models.Owner:connect({},'end',{where={field='status',op='eq',value='UNKNOWN'}}); return true",
        ))
        .err()
        .unwrap();
    assert_eq!(target.member.as_deref(), Some("Target.status"));
    assert_eq!(target.actual, "string outside declared enum");
    let option = contract
        .script(candidate(
            &contract,
            "Query.probe",
            "return #ctx.models.Owner:find({unknownOption=1}) > 0",
        ))
        .err()
        .unwrap();
    assert_eq!(option.member.as_deref(), Some("Owner.find.options"));
    assert_eq!(option.actual, "unsupported option");
    assert!(!format!("{option:?}").contains("unknownOption"));
}

#[test]
fn ab_01_03_private_models_keep_query_and_record_constraints() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "scalar Natural @representation(type:INTEGER,min:0) type Query{probe:Boolean!} type Mutation{probe:Boolean!}",
        "enum PrivateStatus{OPEN CLOSED} type Ledger{id:ID! balance:Float! optional:Float status:PrivateStatus! count:Natural!}",
    );
    check(
        &contract,
        "Query.probe",
        r#"local row=ctx.models.Ledger:find({
            where={all={{field='status',op='eq',value='OPEN'},{field='count',op='gte',value=0}}},
            orderBy={{field='balance',direction='desc'},{field='id',direction='asc'}},
        })[1]
        if row == nil then return false end
        return row.balance + row.count > 0"#,
    );
    check(
        &contract,
        "Mutation.probe",
        "local row=ctx.models.Ledger:create({id='x',balance=1,status='OPEN',count=1}); return row.balance > 0",
    );
    assert!(
        contract
            .declarations()
            .contains("type ChigiriLedgerConcreteRecord = ChigiriLedgerRecord &")
    );
    for body in [
        "ctx.models.Ledger:create({id='x',balance=1}); return true",
        "ctx.models.Ledger:create({id='x',balance=1,optional=ctx.null,status='OPEN',count='one'}); return true",
        "ctx.models.Ledger:update({}, {id='changed'}); return true",
        "local row:ChigiriLedgerConcreteRecord={id='x',balance=1,optional=ctx.null,status='OPEN' :: ChigiriPrivateStatusValue,count=1}; return true",
        "return ctx.models.Ledger:count({where={field='status',op='eq',value='UNKNOWN'}}) > 0",
    ] {
        reject(&contract, "Mutation.probe", body);
    }
}

#[test]
fn ab_02_constant_bracket_option_spellings_receive_the_same_context() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "enum Status{OPEN CLOSED} type Item{id:ID! rank:Int! status:Status!} type Query{probe:Boolean!}",
        "",
    );
    for options in [
        r#"["wh\101re"]={["fi\101ld"]="st\97tus",["\111p"]="\101q",["v\97lue"]="O\80EN"}"#,
        "[ [[where]] ]={[ [[field]] ]=[[status]],[ [[op]] ]=[[eq]],[ [[value]] ]=[[OPEN]]}",
    ] {
        check(
            &contract,
            "Query.probe",
            &format!("local row=ctx.models[ [[Item]] ]:find({{{options}}})[1]; return row ~= nil"),
        );
    }
    let unrelated = ScriptCandidate {
        source: "local model={find=function(_, options) return options end}; return model:find({where={field='status',op='eq',value='OPEN'}})".into(),
        ..candidate(&contract, "Query.probe", "return true")
    };
    let narrowed = super::literal_precision::narrow(&unrelated.source, contract.ir()).0;
    assert_eq!(narrowed, unrelated.source);
}

#[test]
fn ab_02_query_context_preserves_vararg_scope() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID!} type Query{probe:Boolean!}", "");
    check(
        &contract,
        "Query.probe",
        r#"local function read(...: number): number
            return #ctx.models.Item:find({limit = ...})
        end
        return read(1) >= 0"#,
    );
}

#[test]
fn ab_02_query_context_finds_calls_before_later_methods() {
    let _case = crate::vm_gate::test_case();
    let contract = contract("type Item{id:ID!} type Query{probe:Boolean!}", "");
    check(
        &contract,
        "Query.probe",
        r#"return ctx.models.Item:find({
            orderBy={{field="id",direction="desc"}},
        })[1].id:lower() ~= """#,
    );
}
