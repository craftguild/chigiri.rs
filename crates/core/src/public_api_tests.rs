use crate::{
    FieldError, GraphQlError, GraphQlRequest, GraphQlResponse, Location, Object, PathSegment, Value,
};
use std::error::Error;
use std::fmt;

#[test]
fn request_distinguishes_variables_from_extensions() {
    let mut variables = Object::new();
    variables.insert("id", "book-01");
    let mut extensions = Object::new();
    extensions.insert("trace", true);

    let request = GraphQlRequest::new("query Book { book { id } }")
        .with_operation_name("Book")
        .with_variables(variables)
        .with_extensions(extensions);

    assert_eq!(request.operation_name.as_deref(), Some("Book"));
    assert_eq!(request.variables.get("id"), Some(&Value::from("book-01")));
    assert_eq!(request.extensions.get("trace"), Some(&Value::from(true)));
}

#[test]
fn distinguishes_request_errors_from_executed_null_responses() {
    let request_error = GraphQlResponse::request_error(GraphQlError::new("invalid request"));
    let executed_null = GraphQlResponse::executed(Value::Null);

    assert_eq!(request_error.data, None);
    assert_eq!(executed_null.data, Some(Value::Null));
}

#[test]
fn public_field_errors_expose_messages_and_metadata() {
    let error = FieldError::public("invalid name")
        .code("BAD_USER_INPUT")
        .extension("argument", "name")
        .to_graphql_error(
            vec![Location::new(2, 3)],
            vec![PathSegment::from("renameUser")],
        );

    assert_eq!(error.message, "invalid name");
    assert_eq!(
        error.extensions.get("code"),
        Some(&Value::from("BAD_USER_INPUT"))
    );
    assert_eq!(error.extensions.get("argument"), Some(&Value::from("name")));
    assert_eq!(error.locations, [Location::new(2, 3)]);
}

#[test]
fn public_field_error_cannot_overwrite_reservation_error_id() {
    let error = FieldError::public("invalid")
        .extension("errorId", "application-value")
        .to_graphql_error(Vec::new(), Vec::new());
    assert_eq!(error.extensions.get("errorId"), None);
}

#[derive(Debug)]
struct DatabaseError;

impl fmt::Display for DatabaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("database password leaked")
    }
}

impl Error for DatabaseError {}

#[test]
fn internal_field_error_keeps_the_original_error_and_masks_the_client_expression() {
    let field_error = FieldError::internal(DatabaseError)
        .code("LEAKED_CODE")
        .extension("secret", "value");
    assert!(field_error.is_internal());
    assert_eq!(
        field_error.internal_source().unwrap().to_string(),
        "database password leaked"
    );

    let client_error = field_error.to_graphql_error(Vec::new(), Vec::new());
    assert_eq!(client_error.message, "Internal server error");
    assert_eq!(
        client_error.extensions.get("code"),
        Some(&Value::from("INTERNAL_SERVER_ERROR"))
    );
    assert!(!client_error.extensions.contains_key("secret"));
    assert!(!format!("{field_error:?}").contains("database password leaked"));
}
