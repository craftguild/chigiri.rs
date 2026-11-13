//! Verify that descriptions do not change executable semantics.

use chigiri_runtime::{GraphQlRequest, Object};

use super::{block_on, schema};

#[test]
fn operation_variable_fragment_description_does_not_change_the_execution_results() {
    let described = r#"
        "operation description"
        query Named("variable description" $value: String!) {
          echo(value: $value)
          ...Bits
        }
        "fragment description"
        fragment Bits on Query { hello }
    "#;
    let plain = r#"
        query Named($value: String!) {
          echo(value: $value)
          ...Bits
        }
        fragment Bits on Query { hello }
    "#;
    let mut variables = Object::new();
    variables.insert("value", "same");
    let described = block_on(schema().execute(
        GraphQlRequest::new(described).with_variables(variables.clone()),
        (),
    ));
    let plain =
        block_on(schema().execute(GraphQlRequest::new(plain).with_variables(variables), ()));

    assert_eq!(described, plain);
    assert!(described.errors.is_empty());
}
