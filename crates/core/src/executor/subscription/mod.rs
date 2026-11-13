//! Runtime-neutral setup and per-event execution for subscription root fields.

mod event;
mod input;
mod metadata_charge;
mod operation;
mod plan;
mod preflight;
mod setup;

pub use event::{SubscriptionEventDispatch, execute_subscription_event};
pub use input::SubscriptionExecutionInput;
pub use metadata_charge::SubscriptionMetadataCharge;
pub use operation::{PreparedOperation, PreparedSubscriptionOperation};
pub use plan::{PreparedSubscriptionPlan, PreparedSubscriptionRoot, SubscriptionSetupOutcome};
pub use preflight::prepare_subscription_plan;
pub use setup::{
    SubscriptionSetupDispatch, establish_subscription_source, prepare_subscription_root,
};

#[cfg(test)]
mod tests;
