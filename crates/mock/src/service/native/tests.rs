use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chigiri::core::{Object, Source, Value};

use super::execute;
use crate::cancellation::Cancellation;
use crate::ir::MockIr;
use crate::output::{ObjectData, OutputValue};
use crate::runtime::{Execution, ExecutionBudget};
use crate::store::{CallScope, Store};

#[test]
fn captured_parent_returns_old_stored_fields_and_fresh_related_records() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "native-captured.graphql",
            r#"
            type User { id: ID! name: String! posts: [Post!]! @relation }
            type Post {
              id: ID!
              title: String!
              author: User! @relation(fields:["authorId"],references:["id"])
            }
            type Query { posts: [Post!]! }
            "#,
        )])
        .unwrap(),
    );
    let user_ty = ir.graph().type_id("User").unwrap();
    let post_ty = ir.graph().type_id("Post").unwrap();
    let user = ir.model_for(user_ty).unwrap();
    let post = ir.model_for(post_ty).unwrap();
    let title = ir.graph().types()[post_ty.0].fields["title"];
    let author = ir.graph().types()[post_ty.0].fields["author"];
    let mut store = Store::seeded(
        ir.clone(),
        &Object::from_iter([
            (
                "User",
                Value::List(vec![Value::Object(Object::from_iter([
                    ("id", Value::from("u")),
                    ("name", Value::from("old-user")),
                ]))]),
            ),
            (
                "Post",
                Value::List(vec![Value::Object(Object::from_iter([
                    ("id", Value::from("p")),
                    ("title", Value::from("old-title")),
                    ("authorId", Value::from("u")),
                ]))]),
            ),
        ]),
    )
    .unwrap();
    let old = store.snapshot();
    let values = old
        .record(post, "p")
        .unwrap()
        .values()
        .map(|(slot, value)| (slot, value.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut update = store.begin(CallScope::MutationRoot).unwrap();
    update
        .update(
            user,
            &select("u"),
            &Object::from_iter([("name", Value::from("fresh-user"))]),
        )
        .unwrap();
    let snapshot = store.commit(update.prepare().unwrap()).unwrap();
    let OutputValue::Object(parent) =
        crate::output::captured_record(snapshot, post_ty, post, values, Some("p".into())).unwrap()
    else {
        panic!("object required")
    };
    let execution = Execution {
        state: Arc::new(Object::new()),
        request: None,
        deadline: Instant::now() + Duration::from_secs(1),
        cancel: Cancellation::default(),
        shutdown: Cancellation::default(),
        budget: ExecutionBudget::default(),
    };
    assert!(matches!(
        resolved(execute(title, &Object::new(), &parent, &execution)),
        OutputValue::Scalar(Value::String(value)) if value == "old-title"
    ));
    let OutputValue::Object(author) =
        resolved(execute(author, &Object::new(), &parent, &execution))
    else {
        panic!("author required")
    };
    let ObjectData::Model(author) = author.data() else {
        panic!("fresh model required")
    };
    assert_eq!(
        author
            .get(ir.models()[user.0].slots["name"])
            .and_then(Value::as_str),
        Some("fresh-user")
    );
}

fn resolved(result: Result<OutputValue, super::NativeError>) -> OutputValue {
    match result {
        Ok(value) => value,
        Err(_) => panic!("native resolution failed"),
    }
}

fn select(id: &str) -> Object {
    Object::from_iter([(
        "where",
        Value::Object(Object::from_iter([
            ("field", Value::from("id")),
            ("op", Value::from("eq")),
            ("value", Value::from(id)),
        ])),
    )])
}
