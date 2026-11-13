//! Request correlation IDs and sampling decisions.

mod instrumentation;

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::OperationIdentity;

const ID_MAX_BYTES: usize = 64;

macro_rules! opaque_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Eq, Hash, PartialEq)]
        // SDK retention roles share the one charged immutable text allocation.
        pub struct $name(Arc<str>);

        impl $name {
            /// Validates and owns a bounded visible-ASCII value.
            pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > ID_MAX_BYTES
                    || !value.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
                {
                    return Err(IdentityError);
                }
                Ok(Self(Arc::from(value)))
            }

            /// Returns the validated value used for observer and client correlation.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("length", &self.0.len())
                    .finish_non_exhaustive()
            }
        }
    };
}

opaque_id!(RequestId, "One HTTP or direct request is an opaque ID.");
opaque_id!(
    TraceId,
    "Opaque trace ID borrowed from the external tracing backend."
);
opaque_id!(
    ErrorId,
    "One internal failure is the opaque ID that connects the client to the observer."
);

/// The type or byte limit of opaque identity is violated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IdentityError;

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identity must contain 1 to 64 visible ASCII bytes")
    }
}

impl Error for IdentityError {}

macro_rules! factory_error {
    ($name:ident, $message:literal) => {
        pub struct $name(Box<dyn Error + Send + Sync>);

        impl $name {
            pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
                Self(Box::new(source))
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .finish_non_exhaustive()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str($message)
            }
        }

        impl Error for $name {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(self.0.as_ref())
            }
        }
    };
}

factory_error!(ErrorIdFactoryError, "error ID generation failed");
factory_error!(RequestIdFactoryError, "request ID generation failed");

/// Object-safe factory that generates a new error ID for each internal failure.
pub trait ErrorIdFactory: Send + Sync + 'static {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError>;
}

/// Object-safe factory that generates a new request ID for each request.
pub trait RequestIdFactory: Send + Sync + 'static {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError>;
}

/// Sampling decision fixed when a request starts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SamplingDecision {
    Drop,
    #[default]
    Record,
}

/// The contract that determines the sampling time from the request identity.
pub trait Sampler: Send + Sync + 'static {
    fn sample(&self, identity: RequestIdentity<'_>) -> SamplingDecision;
}

/// One request fixed owned correlation and sampling snapshot.
#[derive(Clone, Default)]
pub struct RequestTelemetry {
    request_id: Option<RequestId>,
    trace_id: Option<TraceId>,
    sampling: SamplingDecision,
    instrumentation: Option<instrumentation::State>,
}

impl RequestTelemetry {
    pub fn new(
        request_id: Option<RequestId>,
        trace_id: Option<TraceId>,
        sampling: SamplingDecision,
    ) -> Self {
        Self {
            request_id,
            trace_id,
            sampling,
            instrumentation: None,
        }
    }

    pub const fn request_id(&self) -> Option<&RequestId> {
        self.request_id.as_ref()
    }
    pub const fn trace_id(&self) -> Option<&TraceId> {
        self.trace_id.as_ref()
    }
    pub const fn sampling(&self) -> SamplingDecision {
        self.sampling
    }
    pub const fn identity<'a>(
        &'a self,
        operation: Option<&'a OperationIdentity>,
    ) -> RequestIdentity<'a> {
        RequestIdentity::new(self.request_id(), self.trace_id(), operation)
            .with_sampling(self.sampling)
    }
}

/// Read-only identity view shared throughout the request lifecycle.
#[derive(Clone, Copy)]
pub struct RequestIdentity<'a> {
    request_id: Option<&'a RequestId>,
    trace_id: Option<&'a TraceId>,
    operation: Option<&'a OperationIdentity>,
    sampling: SamplingDecision,
}

impl<'a> RequestIdentity<'a> {
    pub const fn new(
        request_id: Option<&'a RequestId>,
        trace_id: Option<&'a TraceId>,
        operation: Option<&'a OperationIdentity>,
    ) -> Self {
        Self {
            request_id,
            trace_id,
            operation,
            sampling: SamplingDecision::Record,
        }
    }

    pub const fn request_id(&self) -> Option<&'a RequestId> {
        self.request_id
    }
    pub const fn trace_id(&self) -> Option<&'a TraceId> {
        self.trace_id
    }
    pub const fn operation(&self) -> Option<&'a OperationIdentity> {
        self.operation
    }
    pub const fn with_sampling(mut self, sampling: SamplingDecision) -> Self {
        self.sampling = sampling;
        self
    }
    pub const fn sampling(&self) -> SamplingDecision {
        self.sampling
    }
}

impl fmt::Debug for RequestIdentity<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestIdentity")
            .field("has_request_id", &self.request_id.is_some())
            .field("has_trace_id", &self.trace_id.is_some())
            .field("has_operation", &self.operation.is_some())
            .field("sampling", &self.sampling)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{ErrorId, IdentityError, RequestId, TraceId};

    fn shared_value<T: Clone + Eq + std::hash::Hash + std::fmt::Debug>(
        original: T,
        text: impl Fn(&T) -> &str,
    ) {
        use std::hash::Hasher;
        let copied = original.clone();
        assert_eq!(original, copied);
        assert_eq!(text(&original).as_ptr(), text(&copied).as_ptr());
        let hash = |value: &T| {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            value.hash(&mut hash);
            hash.finish()
        };
        assert_eq!(hash(&original), hash(&copied));
        assert!(!format!("{copied:?}").contains("private-value"));
        drop(original);
        assert_eq!(text(&copied), "private-value");
    }

    #[test]
    fn opaque_id_clones_share_validated_text_and_keep_value_and_redaction_contracts() {
        shared_value(RequestId::new("private-value").unwrap(), RequestId::as_str);
        shared_value(TraceId::new("private-value").unwrap(), TraceId::as_str);
        shared_value(ErrorId::new("private-value").unwrap(), ErrorId::as_str);
    }

    #[test]
    fn identity_accepts_bounded_visible_ascii_and_redacts_debug_output() {
        let id = ErrorId::new("internal-1").unwrap();
        assert!(!format!("{id:?}").contains("internal-1"));
        assert_eq!(RequestId::new("bad id"), Err(IdentityError));
        assert!(ErrorId::new("x".repeat(65)).is_err());
    }
}
