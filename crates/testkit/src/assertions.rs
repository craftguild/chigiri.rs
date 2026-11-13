//! Structural GraphQL response assertions without string snapshots.

use std::fmt;

use chigiri::{GraphQlResponse, Value};

/// Asserts that a response is a pre-execution error.
pub fn assert_request_error(response: &GraphQlResponse) -> Result<(), GraphQlAssertionError> {
    if response.data.is_some() || response.errors.is_empty() {
        return Err(GraphQlAssertionError::ExpectedRequestError);
    }
    Ok(())
}

/// Asserts that at least one response error has the exact expected
/// `extensions.code`.
pub fn assert_error_code(
    response: &GraphQlResponse,
    expected: &str,
) -> Result<(), GraphQlAssertionError> {
    if response
        .errors
        .iter()
        .any(|error| error.extensions.get("code") == Some(&Value::from(expected)))
    {
        Ok(())
    } else {
        Err(GraphQlAssertionError::MissingErrorCode)
    }
}

/// Closed failure classification for GraphQL assertions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphQlAssertionError {
    ExpectedRequestError,
    MissingErrorCode,
}

impl fmt::Display for GraphQlAssertionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedRequestError => formatter.write_str("expected a GraphQL request error"),
            Self::MissingErrorCode => formatter.write_str("expected GraphQL error code was absent"),
        }
    }
}

impl std::error::Error for GraphQlAssertionError {}
