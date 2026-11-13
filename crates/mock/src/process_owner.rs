//! Mock shutdown budget over the shared child-tree owner.

use std::process::{Child, Command, ExitStatus};
use std::time::Instant;

pub(crate) use chigiri_host::process::ChildError;

pub(crate) struct OwnedChild(chigiri_host::process::OwnedChild);

impl OwnedChild {
    pub fn spawn(command: &mut Command) -> Result<Self, ChildError> {
        chigiri_host::process::OwnedChild::spawn(command, crate::limits::SHUTDOWN_TIME).map(Self)
    }

    pub fn child(&mut self) -> &mut Child {
        self.0.child()
    }

    #[cfg(test)]
    pub fn exited(&self) -> Result<bool, ChildError> {
        self.0.exited()
    }

    pub fn reap_until(&mut self, deadline: Instant) -> Result<ExitStatus, ChildError> {
        self.0.reap_until(deadline)
    }

    #[doc(hidden)]
    pub(crate) fn process_id(&self) -> u32 {
        self.0.process_id()
    }
}
