//! Capture the origin of the first soft-limit crossing without retaining mutable runtime handles.

use super::request::{LimitKind, LimitNotice};
use chigiri::PathSegment;
use std::sync::Arc;

/// Sanitized location attributed to a limit event.
#[derive(Clone, Debug)]
pub struct LimitOrigin {
    /// Stable phase name such as request, wait, resolver, a hook, or field view.
    pub phase: &'static str,
    /// Schema coordinate such as `Type.field`, when available.
    pub coordinate: Option<Arc<str>>,
    /// Response path validated for publication.
    pub path: Arc<[PathSegment]>,
}

impl Default for LimitOrigin {
    fn default() -> Self {
        Self {
            phase: "request",
            coordinate: None,
            path: Arc::default(),
        }
    }
}

/// One soft-limit warning for one request resource.
#[derive(Clone, Debug)]
pub struct LimitWarning {
    /// Shared resource category.
    pub kind: LimitKind,
    /// Usage that crossed the soft threshold.
    pub observed: usize,
    /// Origin of the operation that crossed the threshold, not of later delivery.
    pub origin: LimitOrigin,
}

impl LimitWarning {
    pub(crate) fn notice(&self) -> LimitNotice {
        LimitNotice {
            kind: self.kind,
            observed: self.observed,
        }
    }
}
