//! Regression tests restricting the cache to validated documents.

use chigiri_runtime::{GraphQlRequest, Object, Value};

use super::{block_on, schema};

#[test]
fn cached_documents_reselect_operations_and_apply_each_requests_variables_independently() {
    let schema = schema();
    let query = r#"
        query Echo($value: String!) { echo(value: $value) }
        query Hello { hello }
    "#;
    let echo = |value: &str| {
        let mut variables = Object::new();
        variables.insert("value", value);
        GraphQlRequest::new(query)
            .with_operation_name("Echo")
            .with_variables(variables)
    };

    let first = block_on(schema.execute(echo("first"), ()));
    let hello =
        block_on(schema.execute(GraphQlRequest::new(query).with_operation_name("Hello"), ()));
    let second = block_on(schema.execute(echo("second"), ()));

    assert_eq!(field(first.data, "echo"), Value::from("first"));
    assert_eq!(field(hello.data, "hello"), Value::from("world"));
    assert_eq!(field(second.data, "echo"), Value::from("second"));
}

fn field(data: Option<Value>, name: &str) -> Value {
    let Value::Object(data) = data.unwrap() else {
        panic!("expected object data");
    };
    data.get(name).cloned().unwrap()
}
