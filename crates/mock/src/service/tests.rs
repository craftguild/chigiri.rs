//! Connect a real worker to the endpoint and verify CRUD, relations, DTOs, and abstract-value completion.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Source};
use chigiri::{BoxFuture, DeadlineFactory, Endpoint, FieldError, GraphQlRequest, GraphQlResponse};
use serde_json::{Value, json};

use super::{Client, Worker, endpoint};
use crate::ir::{FieldId, MockIr};
use crate::luau::{Contract, ScriptCandidate, static_candidates};
use crate::output::OutputValue;
use crate::store::Store;

const BLOG: &str = include_str!("../../fixtures/blog.graphql");
const SEED: &str = r#"{"User":[{"id":"User-1","name":"Ada"},{"id":"User-2","name":"Grace"}],"Post":[{"id":"Post-1","title":"first","body":"body","authorId":"User-1"}]}"#;

pub(super) fn ir(sdl: &str) -> Arc<MockIr> {
    let sdl = sdl
        .replace(" @chigiri_mock_resolver", "")
        .replace(" @chigiri_mock_model", "")
        .replace("@chigiri_mock_value(", "@constraint(");
    Arc::new(MockIr::parse(vec![Source::new("domain.graphql", &sdl)]).unwrap())
}

fn field(ir: &MockIr, coordinate: &str) -> FieldId {
    let (parent, field) = coordinate.split_once('.').unwrap();
    ir.graph().types()[ir.graph().type_id(parent).unwrap().0].fields[field]
}

pub(super) fn candidates(ir: &MockIr, edits: &[(&str, &str)]) -> Vec<ScriptCandidate> {
    let mut candidates = static_candidates(ir).unwrap();
    for (coordinate, body) in edits {
        let id = ir
            .scripts()
            .iter()
            .position(|p| p.field == field(ir, coordinate))
            .unwrap();
        candidates[id].source =
            crate::luau::render_module(ir, candidates[id].script, body).unwrap();
    }
    candidates
}

fn start(sdl: &str, seed: &str, edits: &[(&str, &str)]) -> Worker {
    let ir = ir(sdl);
    let contract = Contract::new(ir.clone()).unwrap();
    let candidates = super::blog_fixture::candidates(sdl, &ir, edits);
    Worker::start(
        contract,
        candidates,
        Store::from_seed_json(ir, seed).unwrap(),
    )
    .unwrap()
}

pub(super) fn bound(client: Client) -> Endpoint<Client> {
    endpoint(client, false, |_, error| {
        FieldError::public(error.to_string()).code("MOCK_RESOLVER_ERROR")
    })
    .unwrap()
}

struct Clock;
impl DeadlineFactory for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

async fn response(endpoint: &Endpoint<Client>, client: &Client, query: &str) -> GraphQlResponse {
    tokio::time::timeout(
        Duration::from_secs(8),
        endpoint.execute(GraphQlRequest::new(query), client.clone(), &Clock),
    )
    .await
    .unwrap()
}

async fn query(endpoint: &Endpoint<Client>, client: &Client, query: &str) -> Value {
    let response = response(endpoint, client, query).await;
    assert!(
        response.errors.is_empty(),
        "query: {query}\nerrors: {:?}",
        response
            .errors
            .iter()
            .map(|error| (&error.message, &error.path))
            .collect::<Vec<_>>()
    );
    chigiri_http::value_to_json(&response.data.unwrap()).unwrap()
}

#[tokio::test]
async fn rsm_05_06_owned_endpoint_executes_blog_crud_and_both_relation_directions() {
    let _test_case = crate::vm_gate::test_case();
    let mut worker = start(BLOG, SEED, &[]);
    let client = &worker.client;
    let endpoint = bound(client.clone());
    assert_eq!(
        query(
            &endpoint,
            client,
            "{ getUsers { id name posts { id author { id } } } }"
        )
        .await,
        json!({"getUsers":[{"id":"User-1","name":"Ada","posts":[{"id":"Post-1","author":{"id":"User-1"}}]},
            {"id":"User-2","name":"Grace","posts":[]}]})
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            r#"mutation { createUser(input:{name:"New"}){id name} }"#
        )
        .await,
        json!({"createUser":{"id":"User-3","name":"New"}})
    );
    assert_eq!(query(&endpoint, client, r#"mutation { createPost(input:{title:"new",body:"body",authorId:"User-2"}){id author{name}} }"#).await,
        json!({"createPost":{"id":"Post-2","author":{"name":"Grace"}}}));
    assert_eq!(
        query(
            &endpoint,
            client,
            r#"mutation { updatePost(id:"Post-1",input:{authorId:"User-2"}){id title author{id}} }"#
        )
        .await,
        json!({"updatePost":{"id":"Post-1","title":"first","author":{"id":"User-2"}}})
    );
    // Reassigning ownership removes the old inverse entry and appends the record to the new owner's ordered list.
    assert_eq!(query(&endpoint, client, r#"{ findPost(authorId:"User-1"){id} all:findPost(authorId:null){id} getUsers{posts{id}} }"#).await,
        json!({"findPost":[],"all":[{"id":"Post-1"},{"id":"Post-2"}],"getUsers":[{"posts":[]},{"posts":[{"id":"Post-2"},{"id":"Post-1"}]},{"posts":[]}]}));
    let invalid = response(&endpoint, client, "{ getUsers { posts(limit:0) { id } } }").await;
    assert_eq!(invalid.data, None);
    assert_eq!(invalid.errors.len(), 1);
    assert_eq!(
        invalid.errors[0].message,
        "Argument `limit` is not defined on `User.posts`"
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            "{ getUsers(limit:0) { id } findPost(limit:0) { id } }"
        )
        .await,
        json!({"getUsers":[],"findPost":[]})
    );
    let rejected = response(
        &endpoint,
        client,
        r#"mutation { updatePost(id:"Post-1",input:{title:null}){id} }"#,
    )
    .await;
    assert_eq!(rejected.errors.len(), 1);
    assert_eq!(
        query(&endpoint, client, r#"mutation { deletePost(id:"Post-1") }"#).await,
        json!({"deletePost":true})
    );
    assert_eq!(
        query(
            &endpoint,
            client,
            r#"{ findPost(id:"Post-1"){id} findPost2:findPost(id:"Post-2"){id} }"#
        )
        .await,
        json!({"findPost":[],"findPost2":[{"id":"Post-2"}]})
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn rsm_07_14_owned_endpoint_preserves_abstract_dto_defaults_oneof_and_json() {
    let _test_case = crate::vm_gate::test_case();
    let sdl = r#"
        scalar Data @representation(type: JSON)
        scalar Count @representation(type: INTEGER)
        interface Node { id: ID! }
        type Packet implements Node { id: ID! data: Data child: Packet label: String! @virtual }
        union Result = Packet
        input Choice @oneOf { text: String count: Int }
        type Query {
            packet: Result!
            node: Node!
            data(value: Data = {}): Data
            count(value: Count = 9007199254740994): Count
            pick(input: Choice!): String!
            strings: [String]!
        }
    "#;
    let packet = "return ctx.models.Packet:find({})[1]";
    let mut worker = start(
        sdl,
        r#"{"Packet":[{"id":"root","child":{"id":"child"},"data":[null]}]}"#,
        &[
            ("Query.packet", packet),
            ("Query.node", packet),
            ("Packet.label", "return parent.id"),
            ("Query.data", "return args.value"),
            ("Query.count", "return args.value"),
            (
                "Query.pick",
                "if args.input.text ~= nil then return args.input.text end return 'count'",
            ),
            ("Query.strings", "return {'a', ctx.null, 'c'}"),
        ],
    );
    let client = &worker.client;
    let endpoint = bound(client.clone());
    let result = query(&endpoint, client, "{ packet { __typename ...Fields } node { id ...Fields } data count pick(input:{text: \"selected\"}) strings } fragment Fields on Packet { data label child { id label } }").await;
    let fragment = json!({"data":[null],"label":"root","child":{"id":"child","label":"child"}});
    let mut packet = fragment.clone();
    packet["__typename"] = json!("Packet");
    let mut node = fragment;
    node["id"] = json!("root");
    assert_eq!(
        result,
        json!({"packet":packet,"node":node,"data":{},"count":9007199254740994_i64,"pick":"selected","strings":["a",null,"c"]})
    );
    for query in [
        "{ pick(input:{}) }",
        "{ pick(input:{text:null}) }",
        "{ pick(input:{text:\"x\",count:1}) }",
        "{ count(value:9007199254740993) }",
    ] {
        assert!(!response(&endpoint, client, query).await.errors.is_empty());
    }
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[tokio::test]
async fn rsm_08_09_worker_keeps_parent_snapshot_and_rejects_foreign_parent_and_context() {
    let _test_case = crate::vm_gate::test_case();
    let mut worker = start(BLOG, SEED, &[]);
    let client = &worker.client;
    let ir = client.ir();
    let args = chigiri_http::value_from_json(json!({"limit":20}))
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    let OutputValue::List(rows) = client
        .execute(field(ir, "Query.findPost"), args, None)
        .await
        .unwrap()
    else {
        panic!()
    };
    let OutputValue::Object(parent) = &rows[0] else {
        panic!()
    };
    let endpoint = bound(client.clone());
    query(
        &endpoint,
        client,
        r#"mutation { updatePost(id:"Post-1",input:{authorId:"User-2"}){id} }"#,
    )
    .await;
    let OutputValue::Object(author) = client
        .execute(
            field(ir, "Post.author"),
            Object::new(),
            Some(parent.clone()),
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let OutputValue::Scalar(name) = client
        .execute(field(ir, "User.name"), Object::new(), Some(author))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(name, chigiri::Value::from("Ada"));
    assert!(
        client
            .execute(field(ir, "User.name"), Object::new(), Some(parent.clone()))
            .await
            .is_err()
    );
    let mut other = start(BLOG, SEED, &[]);
    assert!(
        other
            .client
            .execute(
                field(ir, "Post.author"),
                Object::new(),
                Some(parent.clone())
            )
            .await
            .is_err()
    );
    assert!(
        !response(&endpoint, &other.client, "{ getUsers{id} }")
            .await
            .errors
            .is_empty()
    );
    worker
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
    other
        .stop(Instant::now() + Duration::from_secs(5))
        .await
        .unwrap();
}

#[test]
fn rsm_04_10_worker_accepts_scaffold_and_refuses_invalid_and_foreign_startup() {
    let _test_case = crate::vm_gate::test_case();
    let schema = "type Query { value: Int! @chigiri_mock_resolver }";
    for (body, accepted) in [
        (None, true),
        (Some("return 'wrong'"), false),
        (Some("return 1"), false),
    ] {
        let ir = ir(schema);
        let mut edits = candidates(&ir, &[]);
        if let Some(body) = body {
            edits[0].source = crate::luau::render_module(&ir, edits[0].script, body).unwrap();
        }
        let store = if body == Some("return 1") {
            Store::empty(self::ir(schema))
        } else {
            Store::empty(ir.clone())
        };
        assert_eq!(
            Worker::start(Contract::new(ir).unwrap(), edits, store).is_ok(),
            accepted
        );
    }
}
