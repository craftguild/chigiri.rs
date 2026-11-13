//! Stable request-timeout identity shared by direct and HTTP execution.

use chigiri_runtime::{GraphQlError, GraphQlResponse};

pub(crate) fn request_timeout_response() -> GraphQlResponse {
    GraphQlResponse::request_error(GraphQlError::new("Request timed out").code("REQUEST_TIMEOUT"))
}
