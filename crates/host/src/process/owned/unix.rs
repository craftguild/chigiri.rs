//! Unix process-group owner for a long-lived child.

use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process_group, test_kill_process_group, waitid,
};

use super::ChildError;

/// A child whose process group and final wait remain owned until cleanup.
pub struct OwnedChild {
    child: Option<Child>,
    pid: Pid,
    status: Option<ExitStatus>,
    signalled: bool,
    cleaned: bool,
    cleanup_deadline: Option<Instant>,
    cleanup_time: Duration,
}

impl OwnedChild {
    /// Spawns a process-group leader without releasing cleanup ownership.
    pub fn spawn(command: &mut Command, cleanup_time: Duration) -> Result<Self, ChildError> {
        let child = command.process_group(0).spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ChildError::Unavailable
            } else {
                ChildError::Environment
            }
        })?;
        let pid = Pid::from_raw(child.id() as i32).expect("positive OS child process ID");
        Ok(Self {
            child: Some(child),
            pid,
            status: None,
            signalled: false,
            cleaned: false,
            cleanup_deadline: None,
            cleanup_time,
        })
    }

    /// Borrows the child while its pipes are being configured or transferred.
    pub fn child(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("child process available for recovery")
    }

    /// Observes leader exit without releasing its PID before group cleanup.
    pub fn exited(&self) -> Result<bool, ChildError> {
        if self.status.is_some() {
            return Ok(true);
        }
        match waitid(
            WaitId::Pid(self.pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(status) => Ok(status.is_some()),
            Err(rustix::io::Errno::INTR) => Ok(false),
            Err(_) => Err(ChildError::Cleanup),
        }
    }

    /// Requests termination of the owned process group.
    pub fn signal(&mut self) -> Result<(), ChildError> {
        let deadline = *self
            .cleanup_deadline
            .get_or_insert_with(|| Instant::now() + self.cleanup_time);
        if self.signalled || self.status.is_some() {
            return Ok(());
        }
        loop {
            match kill_process_group(self.pid, Signal::KILL) {
                Ok(()) => {
                    self.signalled = true;
                    return Ok(());
                }
                Err(rustix::io::Errno::SRCH) => {
                    if !self.exited()? {
                        let _ = self.child().kill();
                    }
                    self.signalled = true;
                    return Ok(());
                }
                Err(rustix::io::Errno::PERM) if self.exited()? => {
                    self.signalled = true;
                    return Ok(());
                }
                Err(rustix::io::Errno::PERM | rustix::io::Errno::INTR) => {}
                Err(_) => return Err(ChildError::Cleanup),
            }
            if Instant::now() >= deadline {
                return Err(ChildError::Cleanup);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Terminates the group and confirms leader wait plus descendant absence.
    pub fn reap_until(&mut self, deadline: Instant) -> Result<ExitStatus, ChildError> {
        if self.cleaned {
            return self.status.ok_or(ChildError::Cleanup);
        }
        let deadline = self
            .cleanup_deadline
            .map_or(deadline, |previous| previous.min(deadline));
        self.cleanup_deadline = Some(deadline);
        self.signal()?;
        loop {
            if Instant::now() >= deadline {
                return Err(ChildError::Cleanup);
            }
            if self.status.is_none() && self.exited()? {
                self.status = self.child().try_wait().map_err(|_| ChildError::Cleanup)?;
            }
            if let Some(status) = self.status {
                match test_kill_process_group(self.pid) {
                    Err(rustix::io::Errno::SRCH) => {
                        self.cleaned = true;
                        return Ok(status);
                    }
                    Ok(()) | Err(rustix::io::Errno::INTR) => {}
                    Err(_) => return Err(ChildError::Cleanup),
                }
            }
            if Instant::now() >= deadline {
                return Err(ChildError::Cleanup);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[doc(hidden)]
    pub fn process_id(&self) -> u32 {
        self.pid.as_raw_nonzero().get() as u32
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.cleaned {
            return;
        }
        let deadline = self
            .cleanup_deadline
            .unwrap_or_else(|| Instant::now() + self.cleanup_time);
        if self.reap_until(deadline).is_ok() {
            return;
        }
        let _ = kill_process_group(self.pid, Signal::KILL);
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        std::process::abort();
    }
}
