//! Fixed phase and metadata names for explicitly registered Luau consumers.

pub(crate) const SUBSCRIBE: &str = "subscribe";
pub(crate) const CONSUME: &str = "consume";
pub(crate) const SETUP_SCOPE: &str = "consumer_setup";
pub(crate) const EXECUTION_SCOPE: &str = "consumer";
pub(crate) const ALL_TOPICS: &str = "all_topics";
pub(crate) const EVENT: &str = "event";
pub(crate) const DEPTH: &str = "depth";
pub(crate) const DROPPED_TOTAL: &str = "droppedTotal";
pub(crate) const MAX_DEPTH: u8 = 3;
