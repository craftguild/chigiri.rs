//! HTTP boundaries, lifecycle hooks, and request deadlines.

use std::sync::Arc;
use std::time::Duration;

use chigiri_http::{DecodedRequestHook, ResponseHook, TransportAdmission};

use crate::component_ops;
use crate::draft::Draft;

/// HTTP boundaries, deadlines, CSRF settings.
pub struct HttpComponents<'a, C, S = ()> {
    pub(super) draft: &'a mut Draft<C>,
    pub(super) response_state: std::marker::PhantomData<fn() -> S>,
}

impl<C, S> HttpComponents<'_, C, S>
where
    C: Send + Sync + 'static,
{
    /// Set the maximum byte of request body to less than 1 GiB.
    pub fn body_limit(&mut self, bytes: usize) -> &mut Self {
        component_ops::body_limit(self.draft, bytes);
        self
    }

    /// Sets the encoded-response byte limit, which must be less than 1 GiB.
    pub fn response_byte_limit(&mut self, bytes: usize) -> &mut Self {
        component_ops::response_byte_limit(self.draft, bytes);
        self
    }

    /// Sets a request-body read deadline shorter than 24 hours.
    pub fn body_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.body_timeout,
            timeout,
        );
        self
    }

    /// Set the deadline for Stage A admission in less than 24 hours.
    pub fn transport_admission_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.transport_timeout,
            timeout,
        );
        self
    }

    /// Set the deadline for the operation source solution in less than 24 hours.
    pub fn source_resolution_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.source_timeout,
            timeout,
        );
        self
    }

    /// Set the deadline for decoded request hook in less than 24 hours.
    pub fn decoded_request_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.decoded_timeout,
            timeout,
        );
        self
    }

    /// Set the context factory deadline in less than 24 hours.
    pub fn context_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.context_timeout,
            timeout,
        );
        self
    }

    /// Set the deadline for GraphQL execution in less than 24 hours.
    pub fn execution_timeout(&mut self, timeout: Duration) -> &mut Self {
        component_ops::execution_timeout(self.draft, timeout);
        self
    }

    /// Set the deadline for all HTTP requests in less than 24 hours.
    pub fn overall_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.overall_timeout,
            timeout,
        );
        self
    }

    /// Sets a response-hook deadline shorter than 24 hours.
    pub fn response_hook_timeout(&mut self, timeout: Duration) -> &mut Self {
        Draft::<C>::set(
            &mut self.draft.sequence,
            &mut self.draft.response_hook_timeout,
            timeout,
        );
        self
    }

    /// Appends a decoded-request hook to the execution order.
    pub fn push_decoded_hook(&mut self, hook: impl DecodedRequestHook) -> &mut Self {
        self.draft.mark_change();
        self.draft.decoded.push(Arc::new(hook));
        self
    }

    /// Add transport admission to the end of the run order.
    pub fn push_transport_admission(&mut self, admission: impl TransportAdmission) -> &mut Self {
        self.draft.mark_change();
        self.draft.admissions.push(Arc::new(admission));
        self
    }

    /// Replaces the entire decoded-request hook collection.
    pub fn set_decoded_hooks(&mut self, hooks: Vec<Arc<dyn DecodedRequestHook>>) -> &mut Self {
        self.draft.mark_change();
        self.draft.decoded = hooks;
        self
    }

    /// Clears the decoded-request hook collection.
    pub fn clear_decoded_hooks(&mut self) -> &mut Self {
        self.draft.mark_change();
        self.draft.decoded.clear();
        self
    }

    /// Removes the CSRF/XS-Search guard and records the reason.
    pub fn dangerously_disable_csrf_guard(&mut self, reason: impl Into<Arc<str>>) -> &mut Self {
        self.draft.mark_change();
        self.draft.csrf_enabled = false;
        self.draft.add_override("csrf-guard", reason.into());
        self
    }
}

impl<C, S> HttpComponents<'_, C, S>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    /// Sets the response hook whose state type matches the context factory.
    pub fn response_hook(&mut self, hook: impl ResponseHook<S>) -> &mut Self {
        component_ops::response_hook(self.draft, hook);
        self
    }
}
