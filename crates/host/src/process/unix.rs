//! Polls nonblocking descriptors on the calling thread without leaving pipe workers behind.

#[cfg(test)]
mod tests;

use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::Errno;
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process_group, test_kill_process_group, waitid,
};

use super::{ProcessFailure, ProcessLimits, ProcessOutput};

pub(super) fn run(
    command: &mut Command,
    input: &[u8],
    limits: ProcessLimits,
    cancelled: &impl Fn() -> bool,
) -> Result<ProcessOutput, ProcessFailure> {
    if cancelled() {
        return Err(ProcessFailure::Cancelled);
    }
    if input.len() > limits.bytes {
        return Err(ProcessFailure::Limit);
    }
    let now = Instant::now();
    let deadline = now
        .checked_add(limits.timeout)
        .ok_or(ProcessFailure::Timeout)?;
    let execution = deadline - (limits.timeout / 4).min(Duration::from_secs(1));
    if now >= execution {
        return Err(ProcessFailure::Timeout);
    }
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|_| ProcessFailure::Start)?;
    let mut owner = Owner {
        group: Pid::from_child(&child),
        child,
        deadline,
        cleaned: false,
        status: None,
    };
    let result = communicate(&mut owner, input, limits.bytes, execution, cancelled);
    // The same thread has destroyed the ownership of the pipes in the error path.
    owner.cleanup()?;
    result
}

fn communicate(
    owner: &mut Owner,
    input: &[u8],
    limit: usize,
    execution: Instant,
    cancelled: &impl Fn() -> bool,
) -> Result<ProcessOutput, ProcessFailure> {
    let stdin = owner.child.stdin.take().ok_or(ProcessFailure::Start)?;
    let mut stdout = owner.child.stdout.take().ok_or(ProcessFailure::Start)?;
    let mut stderr = owner.child.stderr.take().ok_or(ProcessFailure::Start)?;
    nonblocking(&stdin)?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut stdin = Some(stdin);
    let mut sent = 0;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut out_eof = false;
    let mut err_eof = false;
    loop {
        if cancelled() {
            return Err(ProcessFailure::Cancelled);
        }
        if Instant::now() >= execution {
            return Err(ProcessFailure::Timeout);
        }
        if sent == input.len() {
            stdin = None;
        }
        let mut progressed = false;
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(&input[sent..input.len().min(sent + 16384)]) {
                Ok(count) => {
                    sent += count;
                    progressed |= count > 0;
                }
                Err(error) if transient(&error) => {}
                Err(_) => return Err(ProcessFailure::Io),
            }
        }
        if sent == input.len() {
            stdin = None;
        }
        if !out_eof {
            let (eof, read) = drain(&mut stdout, &mut out, err.len(), limit)?;
            out_eof = eof;
            progressed |= read;
        }
        if !err_eof {
            let (eof, read) = drain(&mut stderr, &mut err, out.len(), limit)?;
            err_eof = eof;
            progressed |= read;
        }
        if owner.status.is_none() && owner.exited()? {
            // WNOWAIT will signal to the ownership group with a legacy pipe, keeping the leader's PID.
            // After reap, the numerical PGID is not used again. Determining signal failure requires group elimination.
            let _ = signal_group(owner.group);
            owner.status = owner.child.try_wait().map_err(|_| ProcessFailure::Wait)?;
        }
        if let Some(status) = owner.status
            && out_eof
            && err_eof
        {
            if sent != input.len() {
                return Err(ProcessFailure::Io);
            }
            return Ok(ProcessOutput {
                status,
                stdout: out,
                stderr: err,
            });
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn transient(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
    )
}

fn nonblocking(fd: &impl AsFd) -> Result<(), ProcessFailure> {
    let flags = fcntl_getfl(fd).map_err(|_| ProcessFailure::Start)?;
    fcntl_setfl(fd, flags | OFlags::NONBLOCK).map_err(|_| ProcessFailure::Start)
}

fn drain(
    pipe: &mut impl Read,
    retained: &mut Vec<u8>,
    other: usize,
    limit: usize,
) -> Result<(bool, bool), ProcessFailure> {
    let mut buffer = [0; 16384];
    match pipe.read(&mut buffer) {
        Ok(0) => Ok((true, false)),
        Ok(count) => {
            if retained.len().saturating_add(other).saturating_add(count) > limit {
                return Err(ProcessFailure::Limit);
            }
            retained.extend_from_slice(&buffer[..count]);
            Ok((false, true))
        }
        Err(error) if transient(&error) => Ok((false, false)),
        Err(_) => Err(ProcessFailure::Io),
    }
}

struct Owner {
    child: Child,
    group: Pid,
    deadline: Instant,
    cleaned: bool,
    status: Option<ExitStatus>,
}

impl Owner {
    fn exited(&self) -> Result<bool, ProcessFailure> {
        if self.status.is_some() {
            return Ok(true);
        }
        match waitid(
            WaitId::Pid(self.group),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Ok(status) => Ok(status.is_some()),
            Err(Errno::INTR) => Ok(false),
            Err(_) => Err(ProcessFailure::Wait),
        }
    }

    fn cleanup(&mut self) -> Result<(), ProcessFailure> {
        self.cleanup_with_signal(signal_group)
    }

    fn cleanup_with_signal(
        &mut self,
        signal: impl FnOnce(Pid) -> Result<(), ProcessFailure>,
    ) -> Result<(), ProcessFailure> {
        if self.cleaned {
            return Ok(());
        }
        // On macOS, signal delivery can race with reaping and process-group disappearance,
        // including EPERM results. Treat signal failure as advisory and continue recovery
        // until the deadline.
        // The reaped process-group leader PID cannot be reused until this operation completes.
        if self.status.is_none() {
            let _ = signal(self.group);
        }
        loop {
            if self.status.is_none() {
                self.status = self.child.try_wait().map_err(|_| ProcessFailure::Cleanup)?;
            }
            let empty = matches!(test_kill_process_group(self.group), Err(Errno::SRCH));
            if self.status.is_some() && empty {
                self.cleaned = true;
                return Ok(());
            }
            if Instant::now() >= self.deadline {
                return Err(ProcessFailure::Cleanup);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn signal_group(group: Pid) -> Result<(), ProcessFailure> {
    match kill_process_group(group, Signal::KILL) {
        Ok(()) | Err(Errno::SRCH) => Ok(()),
        Err(_) => Err(ProcessFailure::Cleanup),
    }
}

fn require_cleanup(result: Result<(), ProcessFailure>) {
    if result.is_err() {
        std::process::abort();
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        require_cleanup(self.cleanup());
    }
}
