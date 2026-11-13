//! Test result coercion, list-item errors, null propagation, and abstract types.

use chigiri_runtime::__private::{BoxFuture, ResolvedValue};
use chigiri_runtime::{
    ExecutableSchema, FieldContext, FieldNext, FieldResult, GraphQlRequest, Object, PathSegment,
    Plugin, Value,
};

use super::{Dispatch, SCHEMA, block_on, schema};

#[test]
fn null_list_items_preserve_their_index_paths() {
    let response = block_on(schema().execute(GraphQlRequest::new("{ nullableItems }"), ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert_eq!(
        data.get("nullableItems"),
        Some(&Value::List(vec![
            Value::from("first"),
            Value::Null,
            Value::from("third"),
        ]))
    );
    assert_eq!(
        response.errors[0].path,
        [PathSegment::from("nullableItems"), PathSegment::Index(1)]
    );
    assert!(!response.errors[0].locations.is_empty());
}

#[test]
fn non_null_item_bubbles_up_to_nullable_list() {
    let response = block_on(schema().execute(GraphQlRequest::new("{ requiredItems hello }"), ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert_eq!(data.get("requiredItems"), Some(&Value::Null));
    assert_eq!(data.get("hello"), Some(&Value::from("world")));
    assert_eq!(
        response.errors[0].path,
        [PathSegment::from("requiredItems"), PathSegment::Index(1)]
    );
}

#[test]
fn non_null_list_failures_bubble_to_the_root() {
    let response = block_on(schema().execute(GraphQlRequest::new("{ requiredList hello }"), ()));

    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(
        response.errors[0].path,
        [PathSegment::from("requiredList"), PathSegment::Index(1)]
    );
}

#[test]
fn nested_non_null_stops_at_the_nearest_nullable_object() {
    let nullable =
        block_on(schema().execute(GraphQlRequest::new("{ nullableItem { name } hello }"), ()));
    let required =
        block_on(schema().execute(GraphQlRequest::new("{ requiredItem { name } hello }"), ()));
    let Some(Value::Object(data)) = nullable.data else {
        panic!("expected a nullable object response");
    };

    assert_eq!(data.get("nullableItem"), Some(&Value::Null));
    assert_eq!(data.get("hello"), Some(&Value::from("world")));
    assert_eq!(
        nullable.errors[0].path,
        [PathSegment::from("nullableItem"), PathSegment::from("name")]
    );
    assert_eq!(required.data, Some(Value::Null));
    assert_eq!(
        required.errors[0].path,
        [PathSegment::from("requiredItem"), PathSegment::from("name")]
    );
}

#[test]
fn interfaces_and_unions_complete_selections_with_concrete_typenames() {
    let response = block_on(schema().execute(
        GraphQlRequest::new(
            "{ node { __typename name } result { __typename ... on Item { name } } }",
        ),
        (),
    ));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert!(response.errors.is_empty());
    for key in ["node", "result"] {
        let Some(Value::Object(value)) = data.get(key) else {
            panic!("abstract result must be an object");
        };
        assert_eq!(value.get("__typename"), Some(&Value::from("Item")));
        assert_eq!(value.get("name"), Some(&Value::from("abstract")));
    }
}

#[test]
fn builtin_scalar_output_errors_become_masked_field_errors() {
    let executable = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .plugin(InvalidBuiltInResults)
        .build()
        .unwrap();
    let response = block_on(executable.execute(
        GraphQlRequest::new(concat!(
            "{ wrongKind: badScalar nullValue: badScalar ",
            "objectValue: badScalar listValue: badScalar }"
        )),
        (),
    ));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert_eq!(response.errors.len(), 4);
    for key in ["wrongKind", "nullValue", "objectValue", "listValue"] {
        assert_eq!(data.get(key), Some(&Value::Null));
    }
    for error in response.errors {
        assert_eq!(error.message, "Internal server error");
        assert_eq!(
            error.extensions.get("code"),
            Some(&Value::from("INTERNAL_SERVER_ERROR"))
        );
    }
}

struct InvalidBuiltInResults;

impl Plugin<()> for InvalidBuiltInResults {
    fn field<'a>(
        &'a self,
        context: FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if context.field_name() != "badScalar" {
            return next.run(context);
        }
        let value = match context.selection().unwrap().response_key {
            "nullValue" => Value::Null,
            "objectValue" => Value::Object(Object::new()),
            "listValue" => Value::List(Vec::new()),
            _ => Value::Int(1),
        };
        Box::pin(async move { Ok(ResolvedValue::Leaf(value)) })
    }
}

#[test]
fn bubbling_preserves_sibling_errors_completed_before_the_bubble() {
    let response =
        block_on(schema().execute(GraphQlRequest::new("{ requiredError nullableError }"), ()));

    assert_eq!(response.data, Some(Value::Null));
    assert_eq!(response.errors.len(), 2);
    assert_eq!(
        response.errors[0].path,
        [PathSegment::from("requiredError")]
    );
    assert_eq!(
        response.errors[1].path,
        [PathSegment::from("nullableError")]
    );
}
