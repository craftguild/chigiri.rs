//! Test alias grouping, fragments, skip/include directives, and type conditions.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ExecutableSchema, ExecutionLimits, FieldResult, GraphQlRequest, Object, Value,
};

use super::{Dispatch, SCHEMA, block_on};

#[test]
fn skip_include_and_fragment_type_conditions_preserve_source_locations() {
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap();
    let mut variables = Object::new();
    variables.insert("hide", true);
    variables.insert("show", true);
    let request = GraphQlRequest::new(
        r#"
        query Collect($hide: Boolean!, $show: Boolean!) {
          first: hello @skip(if: $hide)
          ...Named @include(if: $show)
          ...Named @skip(if: true)
          item { ... on Item { name } }
        }
        fragment Named on Query { second: hello }
        "#,
    )
    .with_variables(variables);

    let response = block_on(schema.execute(request, ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert!(response.errors.is_empty());
    assert_eq!(
        data.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["second", "item"]
    );
    let Some(Value::Object(item)) = data.get("item") else {
        panic!("expected an item object");
    };
    assert_eq!(item.get("name"), Some(&Value::String("nested".to_owned())));
}

#[test]
fn nullable_null_include_variable_excludes_the_field() {
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap();
    let mut variables = Object::new();
    variables.insert("show", Value::Null);
    let request =
        GraphQlRequest::new("query Q($show: Boolean = true) { hello @include(if: $show) }")
            .with_variables(variables);

    let response = block_on(schema.execute(request, ()));

    assert!(response.errors.is_empty());
    assert_eq!(response.data, Some(Value::Object(Object::new())));
}

#[test]
fn fields_with_the_same_response_key_call_the_resolver_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let schema = ExecutableSchema::builder(&SCHEMA, CountingDispatch(Arc::clone(&calls)))
        .build()
        .unwrap();
    let request = GraphQlRequest::new(
        "query { same: hello, same: hello, ...More } fragment More on Query { same: hello }",
    );

    let response = block_on(schema.execute(request, ()));

    assert!(response.errors.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn deep_fragment_chains_expand_without_call_stack_recursion() {
    let count = 1_000;
    let mut query = String::from("query Deep { ...F0 } ");
    for index in 0..count {
        if index + 1 == count {
            query.push_str(&format!("fragment F{index} on Query {{ hello }} "));
        } else {
            query.push_str(&format!(
                "fragment F{index} on Query {{ ...F{} }} ",
                index + 1
            ));
        }
    }
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .limits(ExecutionLimits::unbounded())
        .build()
        .unwrap();

    let response = block_on(schema.execute(GraphQlRequest::new(query), ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };
    assert!(response.errors.is_empty());
    assert_eq!(data.get("hello"), Some(&Value::from("world")));
}

struct CountingDispatch(Arc<AtomicUsize>);

impl ResolverDispatch<()> for CountingDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        super::dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ResolvedValue::Leaf(Value::String("once".to_owned()))) })
    }
}
