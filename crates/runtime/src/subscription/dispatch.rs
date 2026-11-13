//! Hidden generated and owned-schema setup and event dispatch contracts.

use chigiri_core::FieldResult;
use chigiri_core::executor::{ArgumentValues, BoxFuture, FieldContext, ResolvedValue};

use super::observation::{HostSubscriptionObservation, SubscriptionObservationReceipt};
use super::{ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent};

/// Complete subscription dispatch from the same resolver assembly.
#[doc(hidden)]
pub trait SubscriptionResolverDispatch<C>: Send + Sync + 'static {
    /// Reports the subscription root, borrowing an owned name when necessary.
    fn subscription_root(&self) -> Option<&str>;

    /// Prepares only a host-private, closed observation authority, if installed.
    ///
    /// The host checks its fixed retained bytes before allocating. The receipt
    /// conveys no log identity or emission authority. Generated adapters keep
    /// the default; this is not an application observer registration surface.
    fn prepare_observation(
        &self,
        _context: &C,
        _parent_type: &str,
        _field_name: &str,
        _receipt: SubscriptionObservationReceipt,
        _remaining_metadata_bytes: usize,
    ) -> FieldResult<Option<Box<dyn HostSubscriptionObservation>>> {
        Ok(None)
    }

    /// Establishes one source from the setup context and coerced arguments.
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>>;

    /// Resolves one event with the retained arguments and fresh field context.
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;

    /// Reports a host-retained fatal failure after the full event pipeline.
    ///
    /// This query must read only phase-local typed failure state, without
    /// invoking user code or changing execution state. Ordinary application
    /// errors are not fatal. Generated adapters need no additional capability.
    fn event_execution_failed(&self, _context: &C) -> bool {
        false
    }
}

/// The subscription capability of the single generated root-group adapter.
#[doc(hidden)]
pub trait SubscriptionResolverGroupDispatch<C>: Send + Sync + 'static {
    /// Establishes one source at a validated subscription-root coordinate.
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>>;

    /// Resolves one event at the same generated root coordinate.
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>>;
}
