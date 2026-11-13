//! Tests production policy and the September 2025 introspection query.

use std::fmt::Write;

use chigiri_runtime::{
    ExecutableSchema, GraphQlRequest, GraphQlResponse, IntrospectionPolicy, Object, Value,
};

use super::{Dispatch, SCHEMA, block_on};

#[test]
fn typename_is_always_active_and_only_schema_and_type_are_rejected_by_policy() {
    let schema = ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap();
    let response = block_on(schema.execute(
        GraphQlRequest::new(r#"{ __typename, hidden: __type(name: "Query") { name } }"#),
        (),
    ));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data containing nullable __type");
    };

    assert_eq!(data.get("__typename"), Some(&Value::from("Query")));
    assert_eq!(data.get("hidden"), Some(&Value::Null));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].path, ["hidden".into()]);
    assert_eq!(
        response.errors[0].message,
        "Schema introspection is disabled"
    );

    let schema_error =
        block_on(schema.execute(GraphQlRequest::new("{ __schema { types { name } } }"), ()));
    assert_eq!(schema_error.data, Some(Value::Null));
    assert_eq!(schema_error.errors[0].path, ["__schema".into()]);
}

#[test]
fn type_lookup_resolves_variable_and_type_wrappers_without_an_application_resolver() {
    let schema = enabled_schema();
    let mut variables = Object::new();
    variables.insert("name", "Query");
    let response = block_on(
        schema.execute(
            GraphQlRequest::new(
                r#"
            query Lookup($name: String!) {
              __type(name: $name) {
                kind
                name
                fields { name type { kind name ofType { kind name } } }
              }
            }
            "#,
            )
            .with_variables(variables),
            (),
        ),
    );
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };
    let Some(Value::Object(ty)) = data.get("__type") else {
        panic!("expected a __Type object");
    };

    assert!(response.errors.is_empty());
    assert_eq!(ty.get("kind"), Some(&Value::from("OBJECT")));
    assert_eq!(ty.get("name"), Some(&Value::from("Query")));
    let Some(Value::List(fields)) = ty.get("fields") else {
        panic!("expected a field list");
    };
    let hello = fields
        .iter()
        .filter_map(Value::as_object)
        .find(|field| field.get("name") == Some(&Value::from("hello")))
        .unwrap();
    let Some(Value::Object(hello_type)) = hello.get("type") else {
        panic!("expected a field type");
    };
    assert_eq!(hello_type.get("kind"), Some(&Value::from("NON_NULL")));
}

#[test]
fn type_lookup_uses_the_same_builtins_as_schema_types() {
    let response = block_on(enabled_schema().execute(
        GraphQlRequest::new(
            r#"{
              int: __type(name: "Int") { name }
              float: __type(name: "Float") { name }
              id: __type(name: "ID") { name }
              string: __type(name: "String") { name }
              boolean: __type(name: "Boolean") { name }
            }"#,
        ),
        (),
    ));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert!(response.errors.is_empty(), "{:?}", response.errors);
    for name in ["int", "float", "id"] {
        assert_eq!(data.get(name), Some(&Value::Null), "{name}");
    }
    for (alias, expected) in [("string", "String"), ("boolean", "Boolean")] {
        let Some(Value::Object(ty)) = data.get(alias) else {
            panic!("the alias must resolve to a public type");
        };
        assert_eq!(ty.get("name"), Some(&Value::from(expected)));
    }
}

#[test]
fn null_variable_for_non_null_type_name_argument_produces_a_field_error() {
    let mut variables = Object::new();
    variables.insert("name", Value::Null);
    let request = GraphQlRequest::new(
        r#"query Lookup($name: String = "Query") { __type(name: $name) { name } }"#,
    )
    .with_variables(variables);

    let response = block_on(enabled_schema().execute(request, ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert_eq!(data.get("__type"), Some(&Value::Null));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].path, ["__type".into()]);
}

#[test]
fn null_variable_for_non_null_include_deprecated_argument_nulls_the_list() {
    let mut variables = Object::new();
    variables.insert("old", Value::Null);
    let request = GraphQlRequest::new(
        r#"query Q($old: Boolean) {
          __type(name: "Query") { fields(includeDeprecated: $old) { name } }
        }"#,
    )
    .with_variables(variables);

    let response = block_on(enabled_schema().execute(request, ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };
    let Some(Value::Object(ty)) = data.get("__type") else {
        panic!("expected a __Type object");
    };

    assert_eq!(ty.get("fields"), Some(&Value::Null));
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0].path, ["__type".into(), "fields".into()]);
}

#[test]
fn september_2025_standard_introspection_query_executes_successfully() {
    let response = block_on(enabled_schema().execute(GraphQlRequest::new(STANDARD_QUERY), ()));
    assert_introspection_golden(&response);
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };
    let Some(Value::Object(schema)) = data.get("__schema") else {
        panic!("expected a __schema object");
    };

    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert!(matches!(schema.get("types"), Some(Value::List(values)) if !values.is_empty()));
    assert!(matches!(schema.get("directives"), Some(Value::List(values)) if values.len() >= 5));
    let Some(Value::List(types)) = schema.get("types") else {
        unreachable!();
    };
    let introspection_type = types
        .iter()
        .filter_map(Value::as_object)
        .find(|ty| ty.get("name") == Some(&Value::from("__Type")))
        .unwrap();
    assert_eq!(introspection_type.get("kind"), Some(&Value::from("OBJECT")));
}

fn assert_introspection_golden(response: &GraphQlResponse) {
    let actual = response_snapshot(response);
    let expected = include_str!("../golden/september_2025_introspection.txt");
    assert_eq!(actual, expected);
}

fn response_snapshot(response: &GraphQlResponse) -> String {
    let mut output = String::from("data: ");
    match &response.data {
        Some(value) => write_snapshot_value(value, 0, &mut output),
        None => output.push_str("<undefined>"),
    }
    let _ = write!(output, "\nerrors: {:?}\nextensions: ", response.errors);
    write_snapshot_object(&response.extensions, 0, &mut output);
    output.push('\n');
    output
}

fn write_snapshot_value(value: &Value, indent: usize, output: &mut String) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Boolean(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Int(value) => {
            let _ = write!(output, "{value}");
        }
        Value::BigInt(value) => output.push_str(value),
        Value::Float(value) => {
            let _ = write!(output, "{value:?}");
        }
        Value::String(value) => {
            let _ = write!(output, "{value:?}");
        }
        Value::List(values) => write_snapshot_list(values, indent, output),
        Value::Object(value) => write_snapshot_object(value, indent, output),
    }
}

fn write_snapshot_list(values: &[Value], indent: usize, output: &mut String) {
    output.push('[');
    for value in values {
        output.push('\n');
        push_indent(indent + 2, output);
        write_snapshot_value(value, indent + 2, output);
        output.push(',');
    }
    if !values.is_empty() {
        output.push('\n');
        push_indent(indent, output);
    }
    output.push(']');
}

fn write_snapshot_object(object: &Object, indent: usize, output: &mut String) {
    output.push('{');
    for (name, value) in object {
        output.push('\n');
        push_indent(indent + 2, output);
        let _ = write!(output, "{name:?}: ");
        write_snapshot_value(value, indent + 2, output);
        output.push(',');
    }
    if !object.is_empty() {
        output.push('\n');
        push_indent(indent, output);
    }
    output.push('}');
}

fn push_indent(indent: usize, output: &mut String) {
    output.extend(std::iter::repeat_n(' ', indent));
}

fn enabled_schema() -> ExecutableSchema<()> {
    ExecutableSchema::builder(&SCHEMA, Dispatch)
        .introspection(IntrospectionPolicy::Enabled)
        .build()
        .unwrap()
}

const STANDARD_QUERY: &str = r#"
query IntrospectionQuery {
  __schema {
    description
    queryType { name kind }
    mutationType { name kind }
    subscriptionType { name kind }
    types { ...FullType }
    directives {
      name description isRepeatable locations
      args(includeDeprecated: true) { ...InputValue }
    }
  }
}
fragment FullType on __Type {
  kind name description specifiedByURL
  fields(includeDeprecated: true) {
    name description
    args(includeDeprecated: true) { ...InputValue }
    type { ...TypeRef }
    isDeprecated deprecationReason
  }
  inputFields(includeDeprecated: true) { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) {
    name description isDeprecated deprecationReason
  }
  possibleTypes { ...TypeRef }
  isOneOf
}
fragment InputValue on __InputValue {
  name description type { ...TypeRef } defaultValue isDeprecated deprecationReason
}
fragment TypeRef on __Type {
  kind name
  ofType { kind name ofType { kind name ofType { kind name } } }
}
"#;
