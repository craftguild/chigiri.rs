//! Exercises ID-less values, ULIDs, and root rollback after constraint failures
//! with the new SDL and real VM.

use chigiri::core::Object;

use super::tests::{contract, record, run, script};
use crate::output::{ObjectData, OutputValue};
use crate::store::Store;

#[test]
fn sd_02_unsaved_root_abstract_and_virtual_values_keep_validation_and_snapshot() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "interface Info{label:String!} type PageInfo implements Info{label:String! @constraint(minLength:1) next:PageInfo @virtual} type Query{info:Info!} type Mutation{save:PageInfo!}",
    );
    let query = script(
        &contract,
        "Query.info",
        "return {__typename='PageInfo' :: 'PageInfo', label='plain'}",
    );
    let nested = script(
        &contract,
        "PageInfo.next",
        "return {label=parent.label .. '-next'}",
    );
    let save = script(
        &contract,
        "Mutation.save",
        "return ctx.models.PageInfo:create({label='stored'})",
    );
    let mut store = Store::empty(contract.ir().clone());
    let parent = record(run(&query, &Object::new(), &mut store).unwrap());
    assert!(matches!(parent.data(), ObjectData::Dto(_)));
    assert_eq!(store.snapshot().size().0, 0);
    let stored = record(run(&save, &Object::new(), &mut store).unwrap());
    assert!(matches!(stored.data(), ObjectData::Model(_)));
    let next = record(
        super::execute_nested(
            &nested,
            &Object::new(),
            &parent,
            &Default::default(),
            &Default::default(),
        )
        .unwrap(),
    );
    assert!(matches!(next.data(), ObjectData::Dto(_)));
    assert_eq!(next.snapshot().size().0, 0);
    assert_eq!(store.snapshot().size().0, 1);
    for body in [
        "return {__typename='PageInfo'} :: any",
        "return {__typename='PageInfo', label='', extra=1} :: any",
        "return {__typename='PageInfo', label=''} :: any",
        "return {__typename='PageInfo', label='ok', next={label='forged'}} :: any",
        "return {label='missing discriminator'} :: any",
    ] {
        let invalid = script(&contract, "Query.info", body);
        assert!(run(&invalid, &Object::new(), &mut store).is_err(), "{body}");
        assert_eq!(store.snapshot().size().0, 1);
    }
}

#[test]
fn sd_02_vm_creates_idless_embedded_values_and_virtual_reads_the_same_parent() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        "type Payload{label:String! nodes:[Node!]! computed:String! @virtual} type Node{values:[Payload!]!} type Query{x:Int} type Mutation{make:Payload!}",
    );
    let create = script(
        &contract,
        "Mutation.make",
        "return ctx.models.Payload:create({label = 'hello', nodes = {{values = {}}}})\n",
    );
    let nested = script(&contract, "Payload.computed", "return parent.label\n");
    let mut store = Store::empty(contract.ir().clone());
    let parent = record(run(&create, &Object::new(), &mut store).unwrap());
    let ObjectData::Model(stored) = parent.data() else {
        panic!("expected a stored record");
    };
    assert_eq!(stored.id(), None);
    assert_eq!(stored.values().len(), 2);
    let value = super::execute_nested(
        &nested,
        &Object::new(),
        &parent,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(value, OutputValue::Scalar(value) if value.as_str() == Some("hello")));
    assert_eq!(store.snapshot().size().0, 1);
}

#[test]
fn sd_05_vm_create_type_allows_explicit_identity_and_optional_ulid_input() {
    let _case = crate::vm_gate::test_case();
    let contract =
        contract("type Item{id:ID! name:String!} type Query{x:Int} type Mutation{make:Item!}");
    let explicit = script(
        &contract,
        "Mutation.make",
        "return ctx.models.Item:create({id = 'MixedCASE', name = 'N'})\n",
    );
    let generated = script(
        &contract,
        "Mutation.make",
        "return ctx.models.Item:create({name = 'N'})\n",
    );
    let mut store = Store::empty(contract.ir().clone());
    let output = record(run(&explicit, &Object::new(), &mut store).unwrap());
    let ObjectData::Model(first) = output.data() else {
        panic!("expected a stored record");
    };
    assert_eq!(first.id(), Some("MixedCASE"));
    let output = record(run(&generated, &Object::new(), &mut store).unwrap());
    let ObjectData::Model(second) = output.data() else {
        panic!("expected a stored record");
    };
    let id = second.id().unwrap();
    assert_eq!(id.len(), 26);
    assert_eq!(id, id.to_ascii_lowercase());
    assert!(ulid::Ulid::from_string(id).is_ok());
}

#[test]
fn sd_14_pcall_cannot_commit_after_regex_aggregate_failure_and_next_vm_recovers() {
    let _case = crate::vm_gate::test_case();
    let contract = contract(
        r#"type A{words:[String!]! @constraint(pattern:"^$")} type B{words:[String!]! @constraint(pattern:"^$")} type Query{x:Int} type Mutation{write:Int!}"#,
    );
    let failed = script(
        &contract,
        "Mutation.write",
        r#"
        local words: {string} = {}
        for i=1,1000 do words[i] = "" end
        ctx.models.A:create({words = words})
        ctx.models.B:create({words = words})
        ctx.models.A:create({words = words})
        ctx.models.B:create({words = words})
        local tail: {string} = {}
        for i=1,100 do tail[i] = "" end
        pcall(function() ctx.models.A:create({words = tail}) end)
        return 1
    "#,
    );
    let good = script(
        &contract,
        "Mutation.write",
        r#"
        ctx.models.A:create({words = {""}})
        ctx.models.B:create({words = {""}})
        return 2
    "#,
    );
    let mut store = Store::empty(contract.ir().clone());
    let execution = super::Execution {
        state: contract.state().instantiate().snapshot(),
        request: None,
        deadline: std::time::Instant::now() + super::VM_TIME,
        cancel: Default::default(),
        shutdown: Default::default(),
        budget: Default::default(),
    };
    let snapshot = store.snapshot();
    assert!(
        super::execute_root_with(&failed, &Object::new(), &mut store, &snapshot, &execution,)
            .is_err()
    );
    assert_eq!(execution.budget.0.match_count(), 4096);
    assert_eq!(store.snapshot().size().0, 0);
    assert!(matches!(
        run(&good, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(chigiri::core::Value::Int(2))
    ));
    assert_eq!(store.snapshot().size().0, 2);
}
