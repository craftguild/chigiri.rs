use crate::{ErrorId, GraphQlError, Location, Object, PathSegment, Value};
use std::error::Error;
use std::fmt;

const INTERNAL_MESSAGE: &str = "Internal server error";
const INTERNAL_CODE: &str = "INTERNAL_SERVER_ERROR";

/// Resolver or Field middleware returns the error type.
pub struct FieldError {
    kind: FieldErrorKind,
}

enum FieldErrorKind {
    Public {
        message: String,
        extensions: Object,
    },
    Internal {
        source: Box<dyn Error + Send + Sync>,
        error_id: Option<ErrorId>,
    },
}

/// The standard result type of the field resolver.
pub type FieldResult<T> = Result<T, FieldError>;

impl FieldError {
    /// Creates a client-safe error with a public message and extensions.
    pub fn public(message: impl Into<String>) -> Self {
        Self {
            kind: FieldErrorKind::Public {
                message: message.into(),
                extensions: Object::new(),
            },
        }
    }

    /// Creates an internal error that retains the original error for observers
    /// while exposing a fixed client-visible message.
    pub fn internal(source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            kind: FieldErrorKind::Internal {
                source: Box::new(source),
                error_id: None,
            },
        }
    }

    /// Sets `extensions.code` for a public error.
    ///
    /// Does nothing for internal errors because their fixed client mask cannot be changed.
    pub fn code(mut self, code: impl Into<String>) -> Self {
        if let FieldErrorKind::Public { extensions, .. } = &mut self.kind {
            extensions.insert("code", code.into());
        }
        self
    }

    /// Adds an extension to the public error.
    ///
    /// Extensions are not added to internal errors, preventing metadata leaks.
    pub fn extension(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        if let FieldErrorKind::Public { extensions, .. } = &mut self.kind {
            let key = key.into();
            if key != "errorId" {
                extensions.insert(key, value);
            }
        }
        self
    }

    /// Returns whether this is an internal error.
    pub const fn is_internal(&self) -> bool {
        matches!(self.kind, FieldErrorKind::Internal { .. })
    }

    /// It returns the original internal error that the middleware observer can record.
    pub fn internal_source(&self) -> Option<&(dyn Error + Send + Sync + 'static)> {
        match &self.kind {
            FieldErrorKind::Internal { source, .. } => Some(source.as_ref()),
            FieldErrorKind::Public { .. } => None,
        }
    }

    /// Stores an observer-generated ID on an internal failure that does not yet
    /// have one.
    #[doc(hidden)]
    pub fn assign_error_id(&mut self, error_id: ErrorId) {
        if let FieldErrorKind::Internal {
            error_id: current, ..
        } = &mut self.kind
            && current.is_none()
        {
            *current = Some(error_id);
        }
    }

    /// Returns the internal error ID shared during null bubbling.
    pub fn error_id(&self) -> Option<&ErrorId> {
        match &self.kind {
            FieldErrorKind::Internal { error_id, .. } => error_id.as_ref(),
            FieldErrorKind::Public { .. } => None,
        }
    }

    /// Borrows the client-safe message and string code for bounded observation before copying.
    /// Internal source details, error IDs, and other extensions are omitted.
    pub fn safe_summary(&self) -> (&str, Option<&str>) {
        match &self.kind {
            FieldErrorKind::Public {
                message,
                extensions,
            } => (message, extensions.get("code").and_then(Value::as_str)),
            FieldErrorKind::Internal { .. } => (INTERNAL_MESSAGE, Some(INTERNAL_CODE)),
        }
    }

    /// Converts the field error to its client-visible GraphQL form.
    pub fn to_graphql_error(
        &self,
        locations: Vec<Location>,
        path: Vec<PathSegment>,
    ) -> GraphQlError {
        let (message, extensions) = match &self.kind {
            FieldErrorKind::Public {
                message,
                extensions,
            } => (message.clone(), extensions.clone()),
            FieldErrorKind::Internal { error_id, .. } => {
                let mut extensions = Object::new();
                extensions.insert("code", INTERNAL_CODE);
                if let Some(error_id) = error_id {
                    extensions.insert("errorId", error_id.as_str());
                }
                (INTERNAL_MESSAGE.to_owned(), extensions)
            }
        };
        GraphQlError {
            message,
            locations,
            path,
            extensions,
        }
    }
}

impl fmt::Debug for FieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            FieldErrorKind::Public {
                message,
                extensions,
            } => formatter
                .debug_struct("FieldError")
                .field("visibility", &"public")
                .field("message_bytes", &message.len())
                .field("extension_count", &extensions.len())
                .finish(),
            FieldErrorKind::Internal { .. } => formatter
                .debug_struct("FieldError")
                .field("visibility", &"internal")
                .finish_non_exhaustive(),
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            FieldErrorKind::Public { message, .. } => formatter.write_str(message),
            FieldErrorKind::Internal { .. } => formatter.write_str(INTERNAL_MESSAGE),
        }
    }
}

impl Error for FieldError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            FieldErrorKind::Internal { source, .. } => Some(source.as_ref()),
            FieldErrorKind::Public { .. } => None,
        }
    }
}
