//! Private SDL shares the transaction and model API without entering GraphQL.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::tests::{record, run, script};
use crate::ir::MockIr;
use crate::luau::Contract;
use crate::output::{ObjectData, OutputValue};
use crate::store::Store;

fn private_contract() -> Contract {
    let public = vec![Source::new(
        "schema/001.graphql",
        "type User{id:ID! name:String!} type Query{user:User!} type Mutation{setup:User!}",
    )];
    let storage = vec![Source::new(
        "storage/001.graphql",
        r#"
        extend type User {
          secret: String! @constraint(minLength: 1)
          sessions: [MockSession!]! @relation
        }
        type MockSession {
          id: ID!
          token: String! @constraint(minLength: 2)
          user: User! @relation(fields:["userId"], references:["id"])
        }
        "#,
    )];
    Contract::new(Arc::new(
        MockIr::parse_with_storage(public, storage).unwrap(),
    ))
    .unwrap()
}

#[test]
fn sa_06_private_fields_models_crud_relations_and_constraints_are_atomic() {
    let _case = crate::vm_gate::test_case();
    let contract = private_contract();
    assert!(
        contract
            .ir()
            .public_graph()
            .type_id("MockSession")
            .is_none()
    );
    let user = contract.ir().graph().type_id("User").unwrap();
    assert!(
        !contract.ir().public_graph().types()
            [contract.ir().public_graph().type_id("User").unwrap().0]
            .fields
            .contains_key("secret")
    );
    assert!(
        contract.ir().graph().types()[user.0]
            .fields
            .contains_key("secret")
    );

    let setup = script(
        &contract,
        "Mutation.setup",
        r#"
        local user = ctx.models.User:create({id='u',name='Ada',secret='private'})
        local first = ctx.models.MockSession:create({id='s',token='ok',userId=user.id})
        assert(first.userId == user.id)
        local changed = ctx.models.MockSession:update(
          {where={field='id',op='eq',value='s'}}, {token='next'})[1]
        assert(changed.token == 'next')
        assert(ctx.models.MockSession:delete({where={field='id',op='eq',value='s'}}) == 1)
        ctx.models.MockSession:create({id='s',token='final',userId=user.id})
        return user
        "#,
    );
    let mut store = Store::empty(contract.ir().clone());
    let output = record(run(&setup, &Object::new(), &mut store).unwrap());
    let ObjectData::Model(user_record) = output.data() else {
        panic!("model record required");
    };
    let secret = contract.ir().models()[user_record.model().0].slots["secret"];
    assert_eq!(
        user_record.get(secret),
        Some(&Value::String("private".into()))
    );
    let sessions = contract.ir().graph().types()[user.0].fields["sessions"];
    assert!(matches!(
        output
            .snapshot()
            .reader()
            .related_output(sessions, user_record.row())
            .unwrap(),
        OutputValue::List(values) if values.len() == 1
    ));

    let failed = script(
        &contract,
        "Mutation.setup",
        "ctx.models.User:create({id='other',name='Other',secret='x'}); pcall(function() ctx.models.MockSession:create({id='bad',token='',userId='other'}) end); return ctx.models.User:find({where={field='id',op='eq',value='other'}})[1]",
    );
    assert!(run(&failed, &Object::new(), &mut store).is_err());
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn sa_06_storage_rejects_operations_private_abstracts_and_public_leaks() {
    let public = vec![Source::new(
        "schema.graphql",
        r#"
        directive @tag on OBJECT
        scalar PublicScalar @representation(type: STRING)
        enum PublicStatus { PUBLIC }
        interface PublicNode { id: ID! }
        type PublicObject implements PublicNode { id: ID! }
        union PublicUnion = PublicObject
        input PublicInput { value: Int }
        type Query{value:Int}
        "#,
    )];
    for storage in [
        "extend type Query{hidden:Int}",
        "schema{query:Query}",
        "extend schema{mutation:Query}",
        "interface Hidden{id:ID!}",
        "union HiddenUnion = PublicObject",
        "input HiddenInput { value: Int }",
        "directive @storageOnly on FIELD_DEFINITION",
        "extend enum PublicStatus { STORAGE_ONLY }",
        "extend scalar PublicScalar @specifiedBy(url: \"https://invalid.invalid\")",
        "extend interface PublicNode { hidden: Int }",
        "extend union PublicUnion = PublicObject",
        "extend input PublicInput { hidden: Int }",
        "extend type PublicObject implements PublicNode",
        "extend type PublicObject @tag",
        "type Hidden{id:ID! value(x:Int):Int}",
        "type Hidden{id:ID! value:Int @virtual}",
    ] {
        assert!(
            MockIr::parse_with_storage(
                public.clone(),
                vec![Source::new("storage.graphql", storage)]
            )
            .is_err(),
            "{storage}"
        );
    }
    assert!(
        MockIr::parse(vec![Source::new(
            "schema.graphql",
            "type Query{hidden:Hidden}"
        )])
        .is_err()
    );
}

#[test]
fn sa_06_private_nested_objects_keep_required_storage_fields_and_generated_types() {
    let _case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        "type User{id:ID!} type Query{token:String!} type Mutation{token:String!}",
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "extend type User{secret:Secret!} type Secret{id:ID! token:String!}",
    )];
    let contract = Contract::new(Arc::new(
        MockIr::parse_with_storage(public, storage).unwrap(),
    ))
    .unwrap();
    let declaration = contract.declarations();
    let secret = declaration
        .split("type ChigiriSecretStored = {")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .expect("private stored declaration");
    assert!(
        secret.contains("[\"id\"]: ChigiriIDValue")
            && secret.contains("[\"token\"]: ChigiriStringValue"),
        "{secret}"
    );

    let invalid =
        crate::store::SeedData::json(r#"{"User":[{"id":"u","secret":{}}],"Secret":[]}"#).unwrap();
    assert_eq!(
        invalid.store(contract.ir().clone()).err().unwrap().message,
        "required DTO field missing"
    );

    let seed = crate::store::SeedData::json(
        r#"{"User":[{"id":"u","secret":{"$ref":{"model":"Secret","key":{"id":"s"}}}}],"Secret":[{"id":"s","token":"stored"}]}"#,
    )
    .unwrap();
    let mut store = seed.store(contract.ir().clone()).unwrap();
    let read = script(
        &contract,
        "Query.token",
        "return ctx.models.User:find({where={field='id',op='eq',value='u'}})[1].secret.token",
    );
    assert!(matches!(
        run(&read, &Object::new(), &mut store).unwrap(),
        OutputValue::Scalar(Value::String(value)) if value == "stored"
    ));

    let invalid_write = script(
        &contract,
        "Mutation.token",
        "local models:any=ctx.models; models.User:create({id='bad',secret={id='bad-secret'}}); return 'unexpected'",
    );
    assert!(run(&invalid_write, &Object::new(), &mut store).is_err());
    assert_eq!(store.snapshot().size().0, 2);
}

#[test]
fn sa_06_private_implementors_remain_storage_only() {
    let _case = crate::vm_gate::test_case();
    let public = vec![Source::new(
        "schema.graphql",
        "interface Node{id:ID!} type Public implements Node{id:ID!} type Query{node:Node}",
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "type Hidden implements Node{id:ID! secret:String!}",
    )];
    let contract = Contract::new(Arc::new(
        MockIr::parse_with_storage(public, storage).unwrap(),
    ))
    .unwrap();
    let declarations = contract.declarations();
    let output = declarations
        .split("type ChigiriNodeAbstract = ")
        .nth(1)
        .and_then(|tail| tail.lines().next())
        .expect("public abstract declaration");
    let stored = declarations
        .split("type ChigiriNodeStoredAbstract = ")
        .nth(1)
        .and_then(|tail| tail.lines().next())
        .expect("stored abstract declaration");
    assert!(!output.contains("Hidden"), "{output}");
    assert!(stored.contains("ChigiriHiddenConcreteStored"), "{stored}");

    let hidden = Value::Object(
        [
            (String::from("__typename"), Value::from("Hidden")),
            (String::from("id"), Value::from("hidden")),
            (String::from("secret"), Value::from("private")),
        ]
        .into_iter()
        .collect(),
    );
    let query = contract.ir().graph().query();
    let field = contract.ir().graph().types()[query.0].fields["node"];
    let snapshot = Store::empty(contract.ir().clone()).snapshot();
    let error = crate::output::validate(
        &snapshot,
        field,
        &hidden,
        &mut crate::value::ValueBudget::default(),
    )
    .err()
    .unwrap();
    assert_eq!(error.0, "private concrete output type");
}
