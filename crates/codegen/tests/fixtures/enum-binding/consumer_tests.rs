use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use chigiri::{Endpoint, FieldNext, FieldResult, GraphQlRequest, PathSegment, Plugin, Value};

use super::{Query, generated};

struct ObserveSource(Arc<Mutex<Vec<(String, String)>>>);

impl Plugin<()> for ObserveSource {
    fn field<'a>(
        &'a self,
        context: chigiri::FieldContext<'a, ()>,
        next: FieldNext<'a, ()>,
    ) -> chigiri::__private::BoxFuture<'a, FieldResult<chigiri::__private::ResolvedValue>> {
        Box::pin(async move {
            let result = next.run(context).await;
            if let Err(error) = &result {
                if let Some(binding) = error.internal_source() {
                    let source = binding.source().expect("expected a binding source.");
                    self.0
                        .lock()
                        .unwrap()
                        .push((binding.to_string(), source.to_string()));
                }
            }
            result
        })
    }
}

fn schema(observed: Arc<Mutex<Vec<(String, String)>>>) -> Endpoint<()> {
    let resolvers =
        generated::Resolvers::builder(|_: chigiri::FieldError| -> chigiri::FieldError {
            panic!("framework enum errors must bypass the application mapper")
        })
        .with_query(Query)
        .build();
    Endpoint::builder(resolvers)
        .push_plugin(ObserveSource(observed))
        .inject(|components| {
            components
                .execution()
                .dangerously_enable_introspection("enum binding fixture introspection");
        })
        .without_context()
        .build()
        .unwrap()
}

#[test]
fn enum_02_04_all_the_coercion_paths_use_the_same_binding() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut variables = chigiri::Object::new();
    variables.insert("value", "ALPHA");
    let request = GraphQlRequest::new(
        r#"query Paths($value: ProtocolKind!) {
          literal: direct(value: ALPHA)
          variable: direct(value: $value)
          defaulted
          list(values: [ALPHA])
          nullableList(values: [null, ALPHA])
          listOutput
          object(input: { nested: { kinds: [ALPHA] } })
          recursive(input: { kind: ALPHA, next: { kind: ALPHA } })
          choice(input: { kind: ALPHA })
          nestedChoice(input: { choice: { nested: { kinds: [ALPHA] } } })
          owned(value: ONE)
        }"#,
    )
    .with_variables(variables);

    let response = execute(&schema(Arc::clone(&observed)), request);
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    for field in [
        "literal",
        "variable",
        "defaulted",
        "list",
        "nullableList",
        "object",
        "recursive",
        "choice",
        "nestedChoice",
    ] {
        assert_eq!(data.get(field), Some(&Value::from("ALPHA")));
    }
    assert_eq!(
        data.get("listOutput"),
        Some(&Value::List(vec![
            Value::from("ALPHA"),
            Value::from("ALPHA")
        ]))
    );
    assert_eq!(data.get("owned"), Some(&Value::from("ONE")));
    assert!(observed.lock().unwrap().is_empty());
}

#[test]
fn enum_05_input_output_mask_the_failure_and_keep_the_path_and_source() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let schema = schema(Arc::clone(&observed));
    assert_input_failures(&schema);
    assert_output_failures(&schema);
    assert_eq!(
        *observed.lock().unwrap(),
        [
            binding_source("input secret"),
            binding_source("input secret"),
            binding_source("input secret"),
            binding_source("output secret"),
        ]
    );
}

fn assert_input_failures(schema: &Endpoint<()>) {
    for (query, field) in [
        ("{ inputFailure: direct(value: BETA) }", "inputFailure"),
        (
            "{ recursiveFailure: recursive(input: { kind: ALPHA, next: { kind: BETA } }) }",
            "recursiveFailure",
        ),
        (
            "{ nestedFailure: nestedChoice(input: { choice: { kind: BETA } }) }",
            "nestedFailure",
        ),
    ] {
        let response = execute(schema, GraphQlRequest::new(query));
        assert_eq!(response.data, Some(Value::Null));
        assert_binding_error(&response, &[PathSegment::from(field)]);
    }
}

fn assert_output_failures(schema: &Endpoint<()>) {
    let output = execute(schema, GraphQlRequest::new("{ outputFailure: output }"));
    let Some(Value::Object(data)) = output.data.as_ref() else {
        panic!("expected null output object data");
    };
    assert_eq!(data.get("outputFailure"), Some(&Value::Null));
    assert_binding_error(&output, &[PathSegment::from("outputFailure")]);

    let list = execute(schema, GraphQlRequest::new("{ listFailure }"));
    assert_eq!(list.data, Some(Value::Null));
    assert_binding_error(
        &list,
        &[PathSegment::from("listFailure"), PathSegment::from(1_usize)],
    );

    let nullable = execute(schema, GraphQlRequest::new("{ nullableListFailure }"));
    let expected = serde_json_value(&[Value::from("ALPHA"), Value::Null]);
    let Some(Value::Object(data)) = nullable.data.as_ref() else {
        panic!("expected null list output object data");
    };
    assert_eq!(data.get("nullableListFailure"), Some(&expected));
    assert_binding_error(
        &nullable,
        &[
            PathSegment::from("nullableListFailure"),
            PathSegment::from(1_usize),
        ],
    );
}

fn serde_json_value(values: &[Value]) -> Value {
    Value::List(values.to_vec())
}

fn binding_source(source: &str) -> (String, String) {
    ("GraphQL enum binding failed".to_owned(), source.to_owned())
}

#[test]
fn introspection_exposes_wire_enums() {
    let response = execute(
        &schema(Arc::new(Mutex::new(Vec::new()))),
        GraphQlRequest::new(r#"{ __type(name: "ProtocolKind") { enumValues { name } } }"#),
    );
    let rendered = format!("{:?}", response.data);
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert!(rendered.contains("ALPHA"), "{rendered}");
    assert!(rendered.contains("BETA"), "{rendered}");
}

fn assert_binding_error(response: &chigiri::GraphQlResponse, path: &[PathSegment]) {
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].message, "Internal server error");
    assert_eq!(response.errors[0].path, path);
    assert_eq!(
        response.errors[0].extensions.get("code"),
        Some(&Value::from("INTERNAL_SERVER_ERROR"))
    );
    let rendered = format!("{:?}", response);
    assert!(!rendered.contains("input secret"), "{rendered}");
    assert!(!rendered.contains("output secret"), "{rendered}");
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

fn execute(schema: &Endpoint<()>, request: GraphQlRequest) -> chigiri::GraphQlResponse {
    block_on(schema.execute(request, (), &NeverDeadline))
}

struct NeverDeadline;

impl chigiri::DeadlineFactory for NeverDeadline {
    fn sleep(&self, _: std::time::Duration) -> chigiri::BoxFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}
