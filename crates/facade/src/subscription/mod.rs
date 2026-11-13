//! Transport-neutral subscription preparation and bounded request ownership.

pub(crate) mod bridge_resources;
pub(crate) mod control;
pub(crate) mod delivery;
pub(crate) mod driver;
pub(crate) mod execution_state;
#[cfg(feature = "subscription-worker")]
pub(crate) mod failure;
pub(crate) mod outbox;
pub(crate) mod preparation;
pub(crate) mod request;
pub(crate) mod settings;
pub(crate) mod state;
#[cfg(feature = "subscription-worker")]
pub(crate) mod worker;
