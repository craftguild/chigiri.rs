//! Closed host lifecycle authority, transferred before source initialization.

mod prepared;
mod receipt;
mod source_delivery;
pub use source_delivery::HostSourceDelivery;
#[cfg(test)]
mod tests;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::task::Context;

use super::SubscriptionOverflow;
pub use receipt::SubscriptionObservationReceipt;

/// One already-selected logical decision, not a new priority order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub enum SubscriptionTermination {
    /// The source ended naturally.
    SourceComplete,
    /// Source or event execution, preparation, or its deadline failed.
    ExecutionFailed,
    /// A delivery bound or progress requirement failed.
    DeliveryFailed,
    /// Ownership, worker, invariant, or unexplained cancellation failed.
    Internal,
    /// The client explicitly cancelled a usable operation.
    ClientCancelled,
    /// The peer or shared transport became unusable.
    TransportClosed,
    /// The application explicitly shut down its server.
    ServerShutdown,
}

impl SubscriptionTermination {
    /// The only permitted automatic log outcome/cause pairs.
    pub const fn log_pair(self) -> (&'static str, &'static str) {
        match self {
            Self::SourceComplete => ("completed", "source_complete"),
            Self::ExecutionFailed => ("failed", "execution_failed"),
            Self::DeliveryFailed => ("failed", "delivery_failed"),
            Self::Internal => ("failed", "internal"),
            Self::ClientCancelled => ("cancelled", "client_cancelled"),
            Self::TransportClosed => ("cancelled", "transport_closed"),
            Self::ServerShutdown => ("cancelled", "server_shutdown"),
        }
    }

    /// Whether this typed decision cancels an entered, unregistered setup.
    pub const fn is_cancellation(self) -> bool {
        matches!(
            self,
            Self::ClientCancelled | Self::TransportClosed | Self::ServerShutdown
        )
    }
}

/// Closed host-only operations behind the existing hidden dispatch boundary.
///
/// Implementations own their private host-issued identity and installed sink.
/// This trait neither constructs a Mock identity nor installs a callback on an
/// Endpoint, source wrapper, or HTTP scope. It accepts no application values.
#[doc(hidden)]
pub trait HostSubscriptionObservation: Send + 'static {
    /// Optional fixed source metadata, distinct from encoded-result evictions.
    fn source_delivery(&self) -> Option<std::sync::Arc<HostSourceDelivery>> {
        None
    }
    /// Exact additional retained bytes, checked by the host before allocation.
    fn retained_metadata_bytes(&self) -> usize;

    /// Drains fixed setup/terminal decision data, after releasing host locks.
    fn flush(&mut self);

    /// Validates the policy selected by this authority's own registration.
    fn registered(&self, overflow: SubscriptionOverflow) -> bool;

    /// Observes a successful whole-unsent eviction and counter increment.
    fn drop_oldest(&mut self, dropped_total: u64);

    /// Selects a logical decision once and drains setup before terminal.
    fn finish(&mut self, decision: SubscriptionTermination, dropped_total: u64);
}

/// The sole outside emission authority; identity receipts are not authorities.
///
/// Only validated preparation can construct this owner. It is deliberately not
/// Clone and has no public constructor, sink setter, or application payload.
///
/// ```compile_fail
/// use chigiri_runtime::__private::SubscriptionObservation;
/// let _ = SubscriptionObservation::new(42, "Subscription.changed", |_| {});
/// ```
#[doc(hidden)]
pub struct SubscriptionObservation {
    host: Option<Box<dyn HostSubscriptionObservation>>,
    receipt: SubscriptionObservationReceipt,
    retained_bytes: usize,
    dropped_total: u64,
}

impl SubscriptionObservation {
    /// Returns only the registered host source's fixed accounting owner.
    pub fn source_delivery(&self) -> Option<std::sync::Arc<HostSourceDelivery>> {
        self.host.as_ref().and_then(|host| {
            catch_unwind(AssertUnwindSafe(|| host.source_delivery()))
                .ok()
                .flatten()
        })
    }
    /// Accepts the transfer after it has reached the outside operation owner.
    pub fn accept(&mut self) -> bool {
        self.host.is_some() && self.receipt.accept()
    }

    /// Charges this fixed owner under the existing retained metadata bound.
    pub const fn retained_metadata_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Arms one coalesced notification before draining current fixed decisions.
    pub fn poll(&mut self, context: &mut Context<'_>) {
        self.receipt.arm_owner(context);
        if let Some(host) = self.host.as_mut() {
            let _ = catch_unwind(AssertUnwindSafe(|| host.flush()));
        }
    }

    /// Checks the same registration receipt and policy before activation.
    pub fn registered(
        &self,
        receipt: Option<&SubscriptionObservationReceipt>,
        overflow: SubscriptionOverflow,
    ) -> bool {
        receipt.is_some_and(|receipt| self.receipt.same(receipt))
            && !self.receipt.is_closed()
            && self.host.as_ref().is_some_and(|host| {
                catch_unwind(AssertUnwindSafe(|| host.registered(overflow))).unwrap_or(false)
            })
    }

    /// Called only after the common controller increments its drop total.
    pub fn drop_oldest(&mut self, dropped_total: u64) {
        if let Some(host) = self.host.as_mut() {
            self.dropped_total = dropped_total;
            let _ = catch_unwind(AssertUnwindSafe(|| host.drop_oldest(dropped_total)));
        }
    }

    /// Consumes logical observation before physical source/worker cleanup.
    pub fn finish(&mut self, decision: SubscriptionTermination, dropped_total: u64) {
        self.receipt.close();
        if let Some(mut host) = self.host.take() {
            // Consume before invoking host code, including during unwind. Neither
            // a panic nor owner Drop can select or enqueue the decision again.
            let _ = catch_unwind(AssertUnwindSafe(move || {
                host.finish(decision, dropped_total);
                drop(host);
            }));
        }
    }

    /// Uses the last successful controller increment already drained outside.
    pub fn finish_current(&mut self, decision: SubscriptionTermination) {
        self.finish(decision, self.dropped_total);
    }
}

impl Drop for SubscriptionObservation {
    fn drop(&mut self) {
        self.finish(SubscriptionTermination::Internal, self.dropped_total);
    }
}
