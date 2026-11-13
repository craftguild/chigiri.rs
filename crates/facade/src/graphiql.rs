//! Fixed UI responses and static routes bound to an explicitly enabled endpoint capability.

mod paths;
mod response;

pub use paths::{PathError, Paths};

/// Immutable UI capability available only from a validated endpoint.
#[derive(Clone, Copy, Debug)]
pub struct GraphiqlView {
    _private: (),
}

impl GraphiqlView {
    pub(crate) const fn enabled() -> Self {
        Self { _private: () }
    }

    /// Returns the HTML or fixed-inventory assets without executing GraphQL or
    /// reading a request body.
    ///
    /// `HEAD` uses `GET`'s headers without a body. Other methods on known paths
    /// return 405; unknown paths return 404.
    pub fn respond(&self, paths: &Paths, method: &str, path: &str) -> crate::ResponseParts {
        if path == paths.ui() {
            if !matches!(method, "GET" | "HEAD") {
                return response::method_not_allowed();
            }
            return response::html(paths, method == "HEAD");
        }
        let asset = path
            .strip_prefix(paths.assets_prefix())
            .and_then(chigiri_graphiql::__asset);
        let Some((content_type, body)) = asset else {
            return response::not_found();
        };
        if !matches!(method, "GET" | "HEAD") {
            return response::method_not_allowed();
        }
        response::content(content_type, body, method == "HEAD")
    }
}
