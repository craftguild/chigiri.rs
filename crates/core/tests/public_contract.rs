use chigiri_core::{
    DiagnosticCode, FieldError, FieldResult, GraphQlError, GraphQlRequest, GraphQlResponse, Id,
    Location, Object, PathSegment, SourceMap, Span, Value,
};

const PARSE_CODE: DiagnosticCode = DiagnosticCode::new("SF1001");

fn public_error_result() -> FieldResult<Value> {
    Err(FieldError::public("missing").code("BAD_USER_INPUT"))
}

#[test]
fn foundation_types_are_usable_from_external_crates() {
    let mut sources = SourceMap::new();
    let source_id = sources.add("query.graphql", "{ id }");
    let span = Span::new(source_id, 2, 4);
    assert_eq!(sources.location(span), Some(Location::new(1, 3)));
    assert_eq!(PARSE_CODE.as_str(), "SF1001");

    let mut variables = Object::new();
    variables.insert("id", Value::from(Id::from("book-01")));
    let request = GraphQlRequest::new("query Book { book { id } }").with_variables(variables);
    assert_eq!(request.variables.get("id"), Some(&Value::from("book-01")));

    let response = GraphQlResponse::executed(Value::Null);
    assert_eq!(response.data, Some(Value::Null));

    let field_error = match public_error_result() {
        Ok(_) => panic!("expected the public error to be preserved"),
        Err(error) => error,
    };
    let error = field_error.to_graphql_error(
        vec![Location::new(1, 3)],
        vec![PathSegment::from("book"), PathSegment::from(0_usize)],
    );
    let response =
        GraphQlResponse::request_error(GraphQlError::new("request")).with_error(error.clone());
    assert_eq!(response.errors[1], error);
}

#[test]
fn hk_11_safe_field_summary_keeps_internal_mask_without_cloning_extensions() {
    let public = FieldError::public("Denied")
        .code("MOCK_REJECTED")
        .extension("secret", "not in summary");
    assert_eq!(public.safe_summary(), ("Denied", Some("MOCK_REJECTED")));
    let internal =
        FieldError::internal(std::io::Error::other("private provider source")).code("forged");
    assert_eq!(
        internal.safe_summary(),
        ("Internal server error", Some("INTERNAL_SERVER_ERROR"))
    );
    let nontext = FieldError::public("Failure").extension("code", 7_i32);
    assert_eq!(nontext.safe_summary(), ("Failure", None));
}
