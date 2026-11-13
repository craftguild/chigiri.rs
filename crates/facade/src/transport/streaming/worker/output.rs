//! Committed initial bytes keep their physical reservation inseparable.

use std::fmt;
use std::time::Duration;

use chigiri_http::{ResponseHeader, ResponseParts};

use super::event::HttpSubscriptionEvents;
use crate::subscription::bridge_resources::HttpSubscriptionPhysicalLease;
use crate::subscription::delivery::{EndpointByteOwner, EventByteCharge};
use crate::subscription::outbox::SubscriptionOutbox;
use crate::subscription::worker::{InitialResponseBufferLease, SubscriptionWorkerRuntimeError};

/// Initial HTTP bytes and their physical charge, never a mutable response owner.
///
/// Official adapters move this owner into their body buffer without copying its
/// bytes and retain it until the last physical body reference is destroyed.
#[doc(hidden)]
pub struct SubscriptionInitialResponse {
    response: ResponseParts,
    _ownership: InitialByteOwnership,
    _physical: Option<HttpSubscriptionPhysicalLease>,
    profile: Option<chigiri_http::SubscriptionTransport>,
}

enum InitialByteOwnership {
    Exact { _charge: EventByteCharge },
    ReservedMaximum { _buffer: InitialResponseBufferLease },
    Admission,
    Empty,
}

impl SubscriptionInitialResponse {
    pub(super) const fn empty(status: u16) -> Self {
        Self {
            response: ResponseParts {
                status,
                headers: Vec::new(),
                body: Vec::new(),
            },
            _ownership: InitialByteOwnership::Empty,
            _physical: None,
            profile: None,
        }
    }

    pub(crate) fn physical_lease(&self) -> Option<HttpSubscriptionPhysicalLease> {
        self._physical.clone()
    }

    pub(super) const fn charged(response: ResponseParts, charge: EventByteCharge) -> Self {
        Self {
            response,
            _ownership: InitialByteOwnership::Exact { _charge: charge },
            _physical: None,
            profile: None,
        }
    }

    pub(super) const fn charged_profile(
        response: ResponseParts,
        charge: EventByteCharge,
        profile: chigiri_http::SubscriptionTransport,
    ) -> Self {
        Self {
            response,
            _ownership: InitialByteOwnership::Exact { _charge: charge },
            _physical: None,
            profile: Some(profile),
        }
    }

    pub(super) const fn charged_stream(
        response: ResponseParts,
        charge: EventByteCharge,
        physical: HttpSubscriptionPhysicalLease,
        profile: chigiri_http::SubscriptionTransport,
    ) -> Self {
        Self {
            response,
            _ownership: InitialByteOwnership::Exact { _charge: charge },
            _physical: Some(physical),
            profile: Some(profile),
        }
    }

    pub(super) const fn reserved(
        response: ResponseParts,
        buffer: InitialResponseBufferLease,
    ) -> Self {
        Self {
            response,
            _ownership: InitialByteOwnership::ReservedMaximum { _buffer: buffer },
            _physical: None,
            profile: None,
        }
    }

    pub(super) const fn admission(response: ResponseParts) -> Self {
        Self {
            response,
            _ownership: InitialByteOwnership::Admission,
            _physical: None,
            profile: None,
        }
    }

    /// Returns the already committed HTTP status.
    pub const fn status(&self) -> u16 {
        self.response.status
    }

    /// Borrows the immutable initial response headers without exposing mutation.
    pub fn headers(&self) -> &[ResponseHeader] {
        &self.response.headers
    }

    /// Identifies a selected HTTP stream profile, including bounded initial errors.
    ///
    /// Official adapters must suppress Content-Length and exact size hints for
    /// this representation even when all error/complete bytes are already known.
    /// This is preparation metadata, never inferred from application headers.
    pub const fn is_streaming_profile(&self) -> bool {
        self.profile.is_some()
    }
}

impl AsRef<[u8]> for SubscriptionInitialResponse {
    fn as_ref(&self) -> &[u8] {
        &self.response.body
    }
}

impl fmt::Debug for SubscriptionInitialResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SubscriptionInitialResponse")
            .field("status", &self.response.status)
            .field("header_count", &self.response.headers.len())
            .field("body_bytes", &self.response.body.len())
            .finish_non_exhaustive()
    }
}

/// One fully prepared response; uncommitted source setup never escapes here.
#[doc(hidden)]
pub enum PreparedHttpSubscription {
    /// Accounting lost its physical owner; close without emitting HTTP bytes.
    Failed(SubscriptionWorkerRuntimeError),
    /// A completed finite or selected-profile error response with no live source.
    Response(SubscriptionInitialResponse),
    /// A committed initial response and the sole continuing event worker.
    Ready(ReadyHttpSubscription),
}

/// Committed initial HTTP ownership and the matching bounded event components.
#[doc(hidden)]
pub struct ReadyHttpSubscription {
    inner: Box<ReadyHttpSubscriptionParts>,
}

struct ReadyHttpSubscriptionParts {
    initial: SubscriptionInitialResponse,
    outbox: SubscriptionOutbox,
    events: HttpSubscriptionEvents,
    configuration: HttpStreamConfiguration,
}

pub(crate) struct HttpStreamConfiguration {
    pub(crate) bytes: EndpointByteOwner,
    pub(crate) transport: chigiri_http::SubscriptionTransport,
    pub(crate) heartbeat: Duration,
    pub(crate) maximum_event_bytes: usize,
    pub(crate) send_no_progress: Duration,
    pub(crate) failure: crate::transport::streaming::supervisor::StreamFailureReporter,
}

impl ReadyHttpSubscription {
    pub(super) fn new(
        initial: SubscriptionInitialResponse,
        outbox: SubscriptionOutbox,
        events: HttpSubscriptionEvents,
        configuration: HttpStreamConfiguration,
    ) -> Self {
        Self {
            inner: Box::new(ReadyHttpSubscriptionParts {
                initial,
                outbox,
                events,
                configuration,
            }),
        }
    }

    pub(crate) fn into_supervision(
        self,
    ) -> (
        SubscriptionInitialResponse,
        SubscriptionOutbox,
        HttpSubscriptionEvents,
        HttpStreamConfiguration,
    ) {
        let ReadyHttpSubscriptionParts {
            initial,
            outbox,
            events,
            configuration,
        } = *self.inner;
        (initial, outbox, events, configuration)
    }

    /// Separates already committed bytes from the sole event supervision owner.
    ///
    /// The supervisor must drive worker and outbox timers independently of body
    /// polling; extracting these owners does not itself establish that driver.
    pub fn into_parts(
        self,
    ) -> (
        SubscriptionInitialResponse,
        SubscriptionOutbox,
        HttpSubscriptionEvents,
    ) {
        let ReadyHttpSubscriptionParts {
            initial,
            outbox,
            events,
            configuration: _,
        } = *self.inner;
        (initial, outbox, events)
    }
}
