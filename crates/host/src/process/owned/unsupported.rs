//! Fail-closed long-lived process owner for unreviewed hosts.

use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use super::ChildError;

/// A placeholder that never spawns without a reviewed tree-owner backend.
pub struct OwnedChild(Unsupported);

struct Unsupported;

impl OwnedChild {
    /// Fails closed because no reviewed process-tree backend exists.
    pub fn spawn(_: &mut Command, _: Duration) -> Result<Self, ChildError> {
        Err(ChildError::Unsupported)
    }

    /// Is unreachable because construction always fails.
    pub fn child(&mut self) -> &mut Child {
        panic!("unsupported child owner has no child")
    }

    /// Fails closed because no child can be observed.
    pub fn exited(&self) -> Result<bool, ChildError> {
        Err(ChildError::Unsupported)
    }

    /// Fails closed because no process tree can be signalled.
    pub fn signal(&mut self) -> Result<(), ChildError> {
        Err(ChildError::Unsupported)
    }

    /// Fails closed because no process tree can be reaped.
    pub fn reap_until(&mut self, _: Instant) -> Result<ExitStatus, ChildError> {
        Err(ChildError::Unsupported)
    }

    #[doc(hidden)]
    pub fn process_id(&self) -> u32 {
        0
    }
}
