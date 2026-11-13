//! Bounded correlation values that never retain a live backend span.

use std::fmt;
use std::sync::Arc;

const MAX_STATE_BYTES: usize = 512;
const MAX_STATE_MEMBERS: usize = 32;

/// Provider-validated span identity, independent of any backend span lifetime.
///
/// The provider uses its standard propagator to validate syntax. This value
/// only enforces the retained representation's fixed identity and size bounds;
/// it does not parse HTTP propagation headers.
#[derive(Clone, Eq, PartialEq)]
pub struct TraceParent {
    trace_id: [u8; 16],
    span_id: [u8; 8],
    flags: u8,
    remote: bool,
    state: Arc<str>,
}

impl TraceParent {
    /// Copies valid nonzero identifiers and bounded, provider-validated state.
    ///
    /// Oversized state is discarded before copying, preserving the valid
    /// parent. A zero identifier has no valid parent and returns `None`.
    pub fn new(
        trace_id: [u8; 16],
        span_id: [u8; 8],
        flags: u8,
        remote: bool,
        state: &str,
    ) -> Option<Self> {
        if trace_id == [0; 16] || span_id == [0; 8] {
            return None;
        }
        let state = if state.len() <= MAX_STATE_BYTES
            && state.is_ascii()
            && state.split(',').take(MAX_STATE_MEMBERS + 1).count() <= MAX_STATE_MEMBERS
        {
            state
        } else {
            ""
        };
        Some(Self {
            trace_id,
            span_id,
            flags,
            remote,
            state: Arc::from(state),
        })
    }

    /// Returns the distributed trace identifier as fixed bytes.
    pub const fn trace_id(&self) -> [u8; 16] {
        self.trace_id
    }

    /// Returns the parent span identifier as fixed bytes.
    pub const fn span_id(&self) -> [u8; 8] {
        self.span_id
    }

    /// Returns the provider's propagation flags without changing sampling.
    pub const fn flags(&self) -> u8 {
        self.flags
    }

    /// Distinguishes an extracted remote parent from a local parent.
    pub const fn is_remote(&self) -> bool {
        self.remote
    }

    /// Returns the bounded provider-validated trace state.
    pub fn trace_state(&self) -> &str {
        &self.state
    }
}

impl fmt::Debug for TraceParent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TraceParent")
            .field("remote", &self.remote)
            .field("state_bytes", &self.state.len())
            .finish_non_exhaustive()
    }
}

/// Immutable propagation values, excluding live spans and application context.
///
/// Suppression is present even without a parent. It is independent of trace
/// flags, lifecycle sampling, and optional correlation capacity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PropagationContext {
    parent: Option<TraceParent>,
    suppressed: bool,
}

impl PropagationContext {
    /// Captures bounded correlation and the independently selected control bit.
    pub const fn new(parent: Option<TraceParent>, suppressed: bool) -> Self {
        Self { parent, suppressed }
    }

    /// Returns the selected parent, if one could be retained safely.
    pub const fn parent(&self) -> Option<&TraceParent> {
        self.parent.as_ref()
    }

    /// Returns standard telemetry suppression, including for parentless work.
    pub const fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    /// Drops optional correlation while retaining the recursion-control bit.
    pub const fn without_parent(&self) -> Self {
        Self::new(None, self.suppressed)
    }

    /// Returns optional correlation bytes in addition to the fixed container.
    ///
    /// Consumers account their fixed metadata layout separately. This charge
    /// is conservative when immutable state storage is shared with another owner.
    pub fn correlation_bytes(&self) -> usize {
        self.parent.as_ref().map_or(0, |parent| {
            std::mem::size_of::<TraceParent>() + parent.state.len()
        })
    }

    /// Retains correlation only when its charge fits an already checked budget.
    ///
    /// The capacity decision precedes cloning any optional retained state.
    pub fn retain_within(&self, available: usize) -> Self {
        if self.correlation_bytes() <= available {
            self.clone()
        } else {
            self.without_parent()
        }
    }
}
