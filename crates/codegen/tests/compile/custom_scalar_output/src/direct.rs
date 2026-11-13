//! Test exact custom-scalar responses through generated resolvers.

use chigiri::{ExecutionLimits, GraphQlRequest, Object, Value};

use crate::fixture::{block_on, built_schema, invalid_builtin_schema, schema};

pub(crate) const EXACT_QUERY: &str = concat!(
    "{ nullValue: json(mode: \"null\") ",
    "booleanValue: json(mode: \"boolean\") ",
    "intValue: json(mode: \"int\") ",
    "bigIntValue: json(mode: \"bigint\") ",
    "floatValue: json(mode: \"float\") ",
    "stringValue: json(mode: \"string\") ",
    "objectList: json(mode: \"object-list\") ",
    "listObject: json(mode: \"list-object\") ",
    "ordered: json(mode: \"ordered\") }"
);

#[test]
fn generated_scalars_preserve_json_types_and_object_order_in_direct_responses() {
    let response = execute(EXACT_QUERY, ExecutionLimits::production());

    assert!(response.errors.is_empty());
    assert_eq!(response.data, Some(expected_exact_data()));
}

#[test]
fn parses_custom_scalar_input_objects_and_lists_and_preserves_their_shape() {
    let literal = execute(
        "{ echoJson(value: {object: [1, null, {ok: true}]}) }",
        ExecutionLimits::production(),
    );
    assert!(literal.errors.is_empty());
    let expected = Value::Object(
        [(
            "echoJson",
            Value::Object(
                [(
                    "object",
                    Value::List(vec![
                        Value::Int(1),
                        Value::Null,
                        Value::Object([("ok", Value::Boolean(true))].into_iter().collect()),
                    ]),
                )]
                .into_iter()
                .collect(),
            ),
        )]
        .into_iter()
        .collect(),
    );
    assert_eq!(literal.data, Some(expected));

    let mut request = GraphQlRequest::new("query($value: Json!) { echoJson(value: $value) }");
    request.variables.insert(
        "value",
        Value::List(vec![Value::Object(
            [("nested", Value::String("ok".to_owned()))]
                .into_iter()
                .collect(),
        )]),
    );
    let variable = block_on(schema(ExecutionLimits::production()).execute(request, ()));
    assert!(variable.errors.is_empty());
    assert_eq!(
        variable.data,
        Some(Value::Object(
            [(
                "echoJson",
                Value::List(vec![Value::Object(
                    [("nested", Value::String("ok".to_owned()))]
                        .into_iter()
                        .collect(),
                )]),
            )]
            .into_iter()
            .collect(),
        ))
    );
}

#[test]
fn built_schema_input_and_custom_scalar_record() {
    let response = block_on(built_schema(ExecutionLimits::production()).execute(
        GraphQlRequest::new("{ echoJson(value: {nested: [1, true]}) }"),
        (),
    ));
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data,
        Some(Value::Object(
            [(
                "echoJson",
                Value::Object(
                    [(
                        "nested",
                        Value::List(vec![Value::Int(1), Value::Boolean(true)]),
                    )]
                    .into_iter()
                    .collect(),
                ),
            )]
            .into_iter()
            .collect(),
        ))
    );
}

#[test]
fn rejects_containers_and_null_for_builtin_scalars() {
    let response = block_on(
        invalid_builtin_schema(ExecutionLimits::production()).execute(
            GraphQlRequest::new(concat!(
                "{ wrongKind: text nullValue: text ",
                "objectValue: text listValue: text }"
            )),
            (),
        ),
    );
    assert_eq!(response.errors.len(), 4);
    let data = response.data.unwrap();
    let data = data.as_object().unwrap();
    for key in ["wrongKind", "nullValue", "objectValue", "listValue"] {
        assert_eq!(data.get(key), Some(&Value::Null));
    }
}

pub(crate) fn execute(query: &str, limits: ExecutionLimits) -> chigiri::GraphQlResponse {
    block_on(schema(limits).execute(GraphQlRequest::new(query), ()))
}

pub(crate) fn expected_exact_data() -> Value {
    Value::Object(
        [
            ("nullValue", Value::Null),
            ("booleanValue", Value::Boolean(true)),
            ("intValue", Value::Int(42)),
            (
                "bigIntValue",
                Value::BigInt("170141183460469231731687303715884105728".to_owned()),
            ),
            ("floatValue", Value::Float(1.5)),
            ("stringValue", Value::String("text".to_owned())),
            (
                "objectList",
                Value::Object(
                    [("items", Value::List(vec![Value::Int(1), Value::Null]))]
                        .into_iter()
                        .collect(),
                ),
            ),
            (
                "listObject",
                Value::List(vec![Value::Object(
                    [("ok", Value::Boolean(true))].into_iter().collect(),
                )]),
            ),
            (
                "ordered",
                Value::Object(
                    [
                        (
                            "zeta",
                            Value::List(vec![Value::String("first".to_owned()), Value::Int(2)]),
                        ),
                        ("alpha", Value::Null),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect::<Object>(),
    )
}
