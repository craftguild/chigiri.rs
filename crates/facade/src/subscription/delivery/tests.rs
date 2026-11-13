//! Focused tests for transport-neutral delivery ownership.

mod cleanup;
mod connection_physical;
mod controller;
mod counter_transition;
mod evaluation;
mod evaluator_property;
mod in_flight;
mod permits;
mod resources;

use crate::subscription::settings::{
    SubscriptionLimits, SubscriptionProtocolSettings, ValidatedSubscriptionSettings,
};

fn settings() -> ValidatedSubscriptionSettings {
    ValidatedSubscriptionSettings::new(
        SubscriptionLimits::default(),
        SubscriptionProtocolSettings::default(),
    )
}

fn constrained_settings(
    limits: impl FnOnce(&mut SubscriptionLimits),
    protocol: impl FnOnce(&mut SubscriptionProtocolSettings),
) -> ValidatedSubscriptionSettings {
    let mut delivery = SubscriptionLimits::default();
    let mut websocket = SubscriptionProtocolSettings::default();
    limits(&mut delivery);
    protocol(&mut websocket);
    ValidatedSubscriptionSettings::new(delivery, websocket)
}
