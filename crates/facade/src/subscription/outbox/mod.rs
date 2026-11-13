//! Opaque bounded event ownership between execution and streaming bridges.

mod candidate;
mod frame;
mod freeze;
#[cfg(any(test, feature = "subscription-worker"))]
mod observations;
mod signal;
mod source_delivery;
mod state;
mod terminal;
mod transitions;

#[cfg(feature = "subscription-worker")]
mod retired;
#[cfg(feature = "subscription-worker")]
pub(crate) use retired::RetiredSendProgress;

pub use frame::{SubscriptionSendFrame, SubscriptionSendProgress};
#[cfg(any(test, feature = "subscription-worker"))]
pub(crate) use observations::SubscriptionDropTotals;
pub use signal::SubscriptionOutboxSignal;
pub use state::{SubscriptionEventReservation, SubscriptionOutbox};

#[cfg(test)]
mod tests;
