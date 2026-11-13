//! Focused opaque outbox behavior and ownership tests.

mod candidate;
mod fixture;
mod freeze;
#[cfg(feature = "subscription-worker")]
mod graceful;
mod observations;
mod ownership;
mod queue;
mod source_delivery;
mod terminal;
mod timers;

#[cfg(feature = "subscription-worker")]
mod retired;
