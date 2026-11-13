//! Bind hook registration, state declarations, and initial values to the generation separately from the SDL digest.

use serde::{Deserialize, Serialize};

use super::digest::Digest;
use super::{ProjectError, metadata};
use crate::hooks::{HookSettings, StateContract};

/// Digest-bound evaluated settings; generated type text is always rederived rather than treated as an authority.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigStamp {
    pub capability_version: String,
    pub hooks: HookSettings,
    pub state_contract_digest: Digest,
    pub clock_mode: String,
    pub clock_now: Option<i64>,
    pub events: crate::events::EventSettings,
}

impl ConfigStamp {
    pub fn new(
        hooks: HookSettings,
        state: &StateContract,
        clock: &crate::clock::ClockSettings,
        events: &crate::events::EventSettings,
    ) -> Result<Self, ProjectError> {
        let bytes = metadata::render(state, metadata::MAX_BYTES)?;
        Ok(Self {
            capability_version: crate::api::runtime::VERSION.into(),
            hooks,
            state_contract_digest: Digest::bytes(bytes.as_bytes()),
            clock_mode: clock.mode().into(),
            clock_now: clock.fixed_value(),
            events: events.clone(),
        })
    }
}
