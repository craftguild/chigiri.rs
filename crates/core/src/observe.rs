//! Observation contracts for lifecycle and internal errors that cannot alter results.

use std::error::Error;
use std::fmt;
use std::time::Duration;

use crate::{ErrorId, PathSegment, RequestIdentity};

/// Closed lifecycle phase shared by HTTP and core execution.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LifecyclePhase {
    ServerBuilt,
    RequestStarted,
    HttpPreflight,
    TransportAdmission,
    BodyRead,
    HttpDecode,
    OperationSourceResolution,
    DecodedRequestHook,
    ContextConstruction,
    RequestPlugin,
    DocumentPreparation,
    OperationAdmission,
    Execution,
    ResponseHook,
    HttpEncode,
    RequestCompleted,
}

/// Closed classification of lifecycle phases.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LifecycleResult {
    Started,
    Succeeded,
    Rejected,
    Failed,
    TimedOut,
    Cancelled,
    CacheHit,
    CacheMiss,
}

/// A bounded lifecycle event synchronously delivered to an observer.
#[derive(Clone, Copy)]
pub struct LifecycleEvent<'a> {
    phase: LifecyclePhase,
    result: LifecycleResult,
    duration: Option<Duration>,
    identity: RequestIdentity<'a>,
    code: Option<&'static str>,
    component: &'static str,
}

impl<'a> LifecycleEvent<'a> {
    /// Create an event from static, bounded metadata.
    pub const fn new(
        phase: LifecyclePhase,
        result: LifecycleResult,
        duration: Option<Duration>,
        identity: RequestIdentity<'a>,
        code: Option<&'static str>,
        component: &'static str,
    ) -> Self {
        Self {
            phase,
            result,
            duration,
            identity,
            code,
            component,
        }
    }

    pub const fn phase(&self) -> LifecyclePhase {
        self.phase
    }
    pub const fn result(&self) -> LifecycleResult {
        self.result
    }
    pub const fn duration(&self) -> Option<Duration> {
        self.duration
    }
    pub const fn identity(&self) -> RequestIdentity<'a> {
        self.identity
    }
    pub const fn code(&self) -> Option<&'static str> {
        self.code
    }
    pub const fn component(&self) -> &'static str {
        self.component
    }
}

impl fmt::Debug for LifecycleEvent<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LifecycleEvent")
            .field("phase", &self.phase)
            .field("result", &self.result)
            .field("duration", &self.duration)
            .field("identity", &self.identity)
            .field("code", &self.code)
            .field("component", &self.component)
            .finish()
    }
}

/// Synchronously observes lifecycle events without interfering with the request.
pub trait LifecycleObserver: Send + Sync + 'static {
    /// Records the event without waiting for I/O or changing the result.
    fn observe(&self, event: LifecycleEvent<'_>);
}

/// An internal error event synchronously observed without exposing its source
/// to the client.
pub struct ErrorEvent<'a> {
    error_id: Option<&'a ErrorId>,
    identity: RequestIdentity<'a>,
    phase: LifecyclePhase,
    code: &'static str,
    source: &'a (dyn Error + Send + Sync + 'static),
    path: Option<&'a [PathSegment]>,
    coordinate: Option<&'a str>,
    component: &'static str,
}

impl<'a> ErrorEvent<'a> {
    #[allow(clippy::too_many_arguments)]
    /// Combines one internal failure source with client-safe metadata.
    pub const fn new(
        error_id: Option<&'a ErrorId>,
        identity: RequestIdentity<'a>,
        phase: LifecyclePhase,
        code: &'static str,
        source: &'a (dyn Error + Send + Sync + 'static),
        path: Option<&'a [PathSegment]>,
        coordinate: Option<&'a str>,
        component: &'static str,
    ) -> Self {
        Self {
            error_id,
            identity,
            phase,
            code,
            source,
            path,
            coordinate,
            component,
        }
    }

    pub const fn error_id(&self) -> Option<&'a ErrorId> {
        self.error_id
    }
    pub const fn identity(&self) -> RequestIdentity<'a> {
        self.identity
    }
    pub const fn phase(&self) -> LifecyclePhase {
        self.phase
    }
    pub const fn code(&self) -> &'static str {
        self.code
    }
    pub const fn source(&self) -> &'a (dyn Error + Send + Sync + 'static) {
        self.source
    }
    pub const fn path(&self) -> Option<&'a [PathSegment]> {
        self.path
    }
    pub const fn coordinate(&self) -> Option<&'a str> {
        self.coordinate
    }
    pub const fn component(&self) -> &'static str {
        self.component
    }
}

impl fmt::Debug for ErrorEvent<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ErrorEvent")
            .field("has_error_id", &self.error_id.is_some())
            .field("identity", &self.identity)
            .field("phase", &self.phase)
            .field("code", &self.code)
            .field("has_path", &self.path.is_some())
            .field("has_coordinate", &self.coordinate.is_some())
            .field("component", &self.component)
            .finish()
    }
}

/// Synchronously observes internal errors without changing the request result.
pub trait ErrorObserver: Send + Sync + 'static {
    /// Sends the internal source to a trusted sink.
    fn observe_error(&self, event: ErrorEvent<'_>);
}
