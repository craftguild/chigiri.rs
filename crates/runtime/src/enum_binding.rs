//! Domain-enum binding failures that retain a private source.

use std::error::Error;
use std::fmt;

/// Failure converting between a generated wire enum and a domain enum.
pub struct EnumBindingError {
    source: Box<dyn Error + Send + Sync>,
}

impl EnumBindingError {
    /// Stores an application error as a source that is not exposed to clients.
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(error),
        }
    }
}

impl fmt::Debug for EnumBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EnumBindingError")
    }
}

impl fmt::Display for EnumBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GraphQL enum binding failed")
    }
}

impl Error for EnumBindingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::EnumBindingError;

    #[derive(Debug)]
    struct Secret;

    impl std::fmt::Display for Secret {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("mapping secret")
        }
    }

    impl std::error::Error for Secret {}

    #[test]
    fn display_and_debug_do_not_expose_source_values() {
        let error = EnumBindingError::new(Secret);

        assert_eq!(error.to_string(), "GraphQL enum binding failed");
        assert_eq!(format!("{error:?}"), "EnumBindingError");
        assert_eq!(error.source().unwrap().to_string(), "mapping secret");
        assert!(!error.to_string().contains("secret"));
        assert!(!format!("{error:?}").contains("secret"));
    }
}
