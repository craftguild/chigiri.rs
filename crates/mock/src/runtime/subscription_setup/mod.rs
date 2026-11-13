//! Execute one subscription source setup and return a sealed Rust binding candidate.

mod binding;
mod context;
mod execution;
mod generator;
#[cfg(test)]
mod token_tests;

pub(super) use binding::BindingCollector;
pub(crate) use binding::SubscriptionBinding;
pub(super) use context::capability;
pub(crate) use execution::{execute, execute_consumer_setup};
