//! Origin-relative routes without external URLs or wildcard collisions.

use std::fmt;

/// Static public paths shared by GraphQL, the UI, and embedded assets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paths {
    graphql: String,
    ui: String,
    assets: String,
}

impl Default for Paths {
    fn default() -> Self {
        Self::new("/graphql", "/graphiql").expect("The path is fixed and non-collision.")
    }
}

impl Paths {
    /// Creates paths from a single route definition, each limited to 4096 bytes.
    ///
    /// Paths allow ASCII alphanumeric characters and `/._~-`, but reject dot
    /// segments, empty segments, and queries. A configured trailing slash is
    /// preserved. Host and Forwarded headers are never consulted.
    pub fn new(graphql: &str, ui: &str) -> Result<Self, PathError> {
        validate(graphql)?;
        validate(ui)?;
        let assets = format!("{}/assets/", ui.trim_end_matches('/'));
        if graphql == ui
            || graphql.starts_with(&assets)
            || graphql == assets.trim_end_matches('/')
            || assets.len() > 4096
        {
            return Err(PathError);
        }
        Ok(Self {
            graphql: graphql.into(),
            ui: ui.into(),
            assets,
        })
    }

    /// Applies one public prefix to both routes exactly once.
    ///
    /// The returned routes are merged into `Router`. A prefix such as `/api`
    /// is a static path; a trailing slash is rejected without implicit
    /// correction.
    pub fn with_prefix(&self, prefix: &str) -> Result<Self, PathError> {
        validate(prefix)?;
        if prefix.ends_with('/') {
            return Err(PathError);
        }
        Self::new(
            &format!("{prefix}{}", self.graphql),
            &format!("{prefix}{}", self.ui),
        )
    }

    /// Path that forwards a routed GraphQL request to the same Endpoint.
    pub fn graphql(&self) -> &str {
        &self.graphql
    }
    /// Returns the UI HTML path.
    pub fn ui(&self) -> &str {
        &self.ui
    }
    /// Public prefix for fixed assets with end slash.
    pub fn assets_prefix(&self) -> &str {
        &self.assets
    }
}

fn validate(path: &str) -> Result<(), PathError> {
    if !path.starts_with('/')
        || path.len() > 4096
        || path.contains("//")
        || !path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'~' | b'-')
        })
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(PathError);
    }
    Ok(())
}

/// Fixed diagnostic for invalid origin-relative paths or route collisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathError;

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("GraphQL and GraphiQL paths must be distinct static origin-relative paths")
    }
}

impl std::error::Error for PathError {}
