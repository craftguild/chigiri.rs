//! Type-check and execute every published reference example, asserting output and store effects independently of its description.

use super::tests::{contract, run, script};
use crate::api::{Effect, examples::EXAMPLES};
use crate::{ir::ModelId, output::OutputValue, store::Store};
use chigiri::core::{Object, Value};

const SDL: &str = r#"
type User {
  id:ID! name:String! rank:Int! @constraint(min:0) note:String
  posts:[Post!]! @relation
}
type Post {id:ID! author:User @relation(fields:["authorId"],references:["id"])}
type Query {number:Int! text:String flag:Boolean!}
type Mutation {number:Int! text:String flag:Boolean!}
"#;
const SEED: &str = r#"{"User":[{"id":"1","name":"Alice","rank":1,"note":null},{"id":"2","name":"Bob","rank":2,"note":"present"}],"Post":[{"id":"1","authorId":null}]}"#;

#[test]
fn mr_16_every_common_example_runs_with_independent_output_and_store_assertions() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    for example in EXAMPLES {
        let (field, expected, count) = match example.id {
            "find.records" | "count.records" | "query.predicates" => ("number", Value::Int(2), 2),
            "create.record" => ("text", Value::Null, 3),
            "update.patch" => ("text", Value::String("Renamed".into()), 2),
            "delete.record" => ("number", Value::Int(1), 1),
            "connect.owner" | "query.page" => ("text", Value::String("1".into()), 2),
            "disconnect.owner" => ("flag", Value::Boolean(true), 2),
            "query.operators" => ("flag", Value::Boolean(true), 2),
            "query.null" => ("number", Value::Int(1), 2),
            "transaction.candidate" => ("number", Value::Int(1), 3),
            "transaction.caught" => ("flag", Value::Boolean(true), 2),
            id => panic!("independent assertion missing for example: {id}"),
        };
        let parent = if example.effect == Effect::Write {
            "Mutation"
        } else {
            "Query"
        };
        let graph = contract.ir().graph();
        let field_id = graph.types()[graph.type_id(parent).unwrap().0].fields[field];
        let script_id = contract
            .ir()
            .scripts()
            .iter()
            .position(|plan| plan.field == field_id)
            .unwrap();
        let candidate = contract
            .script(crate::luau::ScriptCandidate {
                script: crate::ir::ScriptId(script_id),
                source: crate::luau::render_module(
                    contract.ir(),
                    crate::ir::ScriptId(script_id),
                    example.body,
                )
                .unwrap(),
            })
            .unwrap_or_else(|error| panic!("{}: {error}", example.id));
        let mut store = Store::from_seed_json(contract.ir().clone(), SEED).unwrap();
        let before = store.snapshot();
        let result = run(&candidate, &Object::new(), &mut store);
        if example.id == "transaction.caught" {
            assert!(result.is_err());
        } else {
            let OutputValue::Scalar(actual) = result.unwrap() else {
                panic!("scalar return value required")
            };
            if example.id == "create.record" {
                // Validate the runtime-generated ID itself rather than comparing it with a fixed fixture ID.
                let Value::String(id) = actual else {
                    panic!("ULID string required")
                };
                assert_eq!(id.len(), 26);
                assert_eq!(id, id.to_ascii_lowercase());
                assert!(ulid::Ulid::from_string(&id).is_ok());
                let snapshot = store.snapshot();
                let created = snapshot.record(ModelId(0), &id).unwrap();
                assert_eq!(value(&contract, created, "name").as_str(), Some("Chika"));
                assert_eq!(value(&contract, created, "rank"), &Value::Int(3));
            } else {
                assert_eq!(actual, expected, "{}", example.id);
            }
        }
        let after = store.snapshot();
        assert_eq!(
            after.count(ModelId(0), &Object::new()).unwrap(),
            count,
            "{}",
            example.id
        );
        assert_eq!(before.count(ModelId(0), &Object::new()).unwrap(), 2);
        let rows = after.find(ModelId(0), &Object::new()).unwrap();
        let first = &rows[0];
        assert_eq!(first.id().unwrap(), "1");
        assert_eq!(*value(&contract, first, "rank"), Value::Int(1));
        if example.id == "update.patch" {
            assert_eq!(
                *value(&contract, first, "name"),
                Value::String("Renamed".into())
            );
            assert!(value(&contract, first, "note").is_null());
        } else {
            assert_eq!(
                *value(&contract, first, "name"),
                Value::String("Alice".into())
            );
        }
        let posts = after.find(ModelId(1), &Object::new()).unwrap();
        if example.id == "connect.owner" {
            assert_eq!(
                *value(&contract, &posts[0], "authorId"),
                Value::String("1".into())
            );
        } else {
            assert!(value(&contract, &posts[0], "authorId").is_null());
        }
        if example.id == "transaction.caught" {
            let create = script(
                &contract,
                "Mutation.text",
                "return ctx.models.User:create({id='User-1',name='after',rank=3}).id",
            );
            assert!(
                matches!(run(&create, &Object::new(), &mut store).unwrap(), OutputValue::Scalar(Value::String(id)) if id == "User-1")
            );
        }
    }
}

fn value<'a>(
    contract: &crate::luau::Contract,
    record: &'a crate::store::Record,
    name: &str,
) -> &'a Value {
    record
        .get(contract.ir().models()[record.model().0].slots[name])
        .unwrap()
}

#[test]
fn mr_02_executable_example_mutation_is_detected_by_independent_host_result() {
    let _test_case = crate::vm_gate::test_case();
    let contract = contract(SDL);
    let original = EXAMPLES
        .iter()
        .find(|example| example.id == "count.records")
        .unwrap();
    for (body, conforms) in [
        (original.body, true),
        ("return 0", false),
        ("return #ctx.models.User:find({limit=1})", false),
    ] {
        let mut store = Store::from_seed_json(contract.ir().clone(), SEED).unwrap();
        let candidate = script(&contract, "Query.number", body);
        let actual = run(&candidate, &Object::new(), &mut store).unwrap();
        let correct = matches!(actual, OutputValue::Scalar(Value::Int(2)));
        assert_eq!(correct, conforms);
    }
}

#[test]
fn mr_17_schema_specific_examples_use_exact_renamed_and_luau_keyword_model_names() {
    let _test_case = crate::vm_gate::test_case();
    for model in ["Widget", "end"] {
        let sdl = format!("type {model} {{id:ID!}} type Query {{number:Int!}}");
        let contract = contract(&sdl);
        let document = crate::reference::ReferenceDocument::build(Some(contract.ir())).unwrap();
        let bytes = document.render(crate::reference::Format::Json).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let examples: Vec<_> = json["examples"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|example| example["origin"] == "schema_specific")
            .collect();
        assert_eq!(examples.len(), 1);
        let candidate = script(
            &contract,
            "Query.number",
            examples[0]["luau"].as_str().unwrap(),
        );
        let seed =
            serde_json::json!({model:[{"id":"one"}, {"id":"two"}, {"id":"three"}]}).to_string();
        let mut store = Store::from_seed_json(contract.ir().clone(), &seed).unwrap();
        assert!(matches!(
            run(&candidate, &Object::new(), &mut store).unwrap(),
            OutputValue::Scalar(Value::Int(3))
        ));
    }
}

#[test]
fn tl_16_stdlib_reference_example_checks_and_runs_for_a_renamed_schema() {
    let _test_case = crate::vm_gate::test_case();
    let selection = crate::reference::Selection::new(
        Some(crate::reference::Topic::Runtime),
        Some("stdlib".into()),
    )
    .unwrap();
    let bytes = crate::reference::ReferenceDocument::build(None)
        .unwrap()
        .render_selection(crate::reference::Format::Json, &selection)
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let body = json["content"]["items"][0]["examples"][0]["code"]
        .as_str()
        .unwrap();
    let contract =
        contract("scalar Data @representation(type:JSON) type Query { renamedChronology: Data! }");
    let resolver = script(&contract, "Query.renamedChronology", body);
    let mut store = Store::empty(contract.ir().clone());
    let OutputValue::Scalar(Value::Object(result)) =
        run(&resolver, &Object::new(), &mut store).unwrap()
    else {
        panic!("stdlib example must return its documented object")
    };
    assert!(matches!(
        result.get("now"),
        Some(Value::Int(_) | Value::Float(_))
    ));
    let Some(Value::Object(utc)) = result.get("utc") else {
        panic!("stdlib example must return UTC fields")
    };
    assert_eq!(utc.get("year"), Some(&Value::Int(2026)));
}
