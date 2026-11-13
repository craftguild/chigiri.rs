//! Define VM- and transport-independent state plus the six fixed hook contracts.

mod slots;
mod state;
mod state_json;
#[cfg(test)]
mod state_json_tests;
mod state_schema;
#[cfg(test)]
mod state_tests;
mod state_value;

pub use slots::{HookPhase, HookSettings, HookSlot, HookTiming, Registration};
pub use state::{RequestState, StateContract, StateFields, StateType};
mod rejection;
pub use rejection::{Rejection, assigned};
