//! Trusted bridge entry after protocol admission, source resolution, and decoded hooks.

use chigiri_http::{
    ConnectionInitContext, ConnectionInitDecision, ConnectionInitError, RequestMethod,
    ResponseMediaType, SubscriptionTransport,
};
use chigiri_runtime::Object;

use super::*;
use crate::__streaming::SubscriptionInitialization;

#[cfg(test)]
mod tests;

impl<C: Send + Sync + 'static> Endpoint<C> {
    /// Invokes the configured authorization policy after transport admission.
    ///
    /// The bridge must enforce the bounded initialization deadline, payload
    /// limit, and fixed safe protocol mapping around this application callback.
    #[doc(hidden)]
    pub fn __authorize_connection_init<'a>(
        &'a self,
        request: ConnectionInitContext<'a>,
    ) -> BoxFuture<'a, Result<ConnectionInitDecision, ConnectionInitError>> {
        let state = self.neutral.shared_state();
        Box::pin(async move { state.subscriptions.authorizer.authorize(request).await })
    }

    /// Prepares a decoded subscription for an official streaming bridge.
    ///
    /// The bridge must already have applied transport admission, CSRF/origin,
    /// method/media, operation-source, and decoded-request policies. The optional
    /// initialization object must have passed connection authorization. This
    /// internal seam neither performs an HTTP upgrade nor owns a listener.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn __start_subscription<'a>(
        &'a self,
        transport: TransportRequest<'a>,
        request: GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
        profile: SubscriptionTransport,
        authorized_init: Option<Object>,
        deadlines: &'a dyn DeadlineFactory,
    ) -> BoxFuture<'a, SubscriptionInitialization<C>> {
        crate::subscription::driver::start(
            self.neutral.shared_state(),
            transport,
            request,
            method,
            media,
            profile,
            authorized_init,
            deadlines,
            None,
        )
    }

    /// Continues a shared transport request deadline without resetting its budget.
    ///
    /// This has the same admission prerequisites as `__start_subscription`.
    /// HTTP bridges use this form after their existing decode/admission phases.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn __start_subscription_with_deadline<'a, 'deadline: 'a>(
        &'a self,
        transport: TransportRequest<'a>,
        request: GraphQlRequest,
        method: RequestMethod,
        media: ResponseMediaType,
        profile: SubscriptionTransport,
        authorized_init: Option<Object>,
        deadlines: &'a dyn DeadlineFactory,
        overall: std::pin::Pin<&'a mut (dyn Future<Output = ()> + Send + 'deadline)>,
    ) -> BoxFuture<'a, SubscriptionInitialization<C>> {
        crate::subscription::driver::start(
            self.neutral.shared_state(),
            transport,
            request,
            method,
            media,
            profile,
            authorized_init,
            deadlines,
            Some(overall),
        )
    }
}
