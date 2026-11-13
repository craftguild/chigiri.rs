//! Bounded parent-to-pump transport for the same-executable writer child.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{ChildStderr, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{COMMAND, LINE_BYTES};
use crate::process_owner::OwnedChild;

/// A capacity-one transport whose pump owns all blocking pipe I/O.
pub struct Pipe {
    child: Option<OwnedChild>,
    frames: Option<SyncSender<Vec<u8>>>,
    results: Receiver<bool>,
    pump: Option<JoinHandle<()>>,
    stop_deadline: Option<Instant>,
    pending: bool,
    failed: bool,
}

impl Pipe {
    /// Starts a writer child whose standard output is connected to `output`.
    pub fn start(executable: &Path, output: std::io::Result<Stdio>) -> Self {
        let (frames, input) = sync_channel(1);
        let (delivered, results) = sync_channel(1);
        let mut pipe = Self {
            child: None,
            frames: Some(frames),
            results,
            pump: None,
            stop_deadline: None,
            pending: false,
            failed: true,
        };
        let Ok(output) = output else {
            return pipe;
        };
        let Ok(mut child) = OwnedChild::spawn(
            Command::new(executable)
                .arg(COMMAND)
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(output)
                .stderr(Stdio::piped()),
        ) else {
            return pipe;
        };
        let (Some(writer), Some(ack)) = (child.child().stdin.take(), child.child().stderr.take())
        else {
            return pipe;
        };
        let pump = std::thread::Builder::new()
            .name("chigiri-stdio-pump".into())
            .spawn(move || pump(input, delivered, writer, ack));
        let Ok(pump) = pump else {
            return pipe;
        };
        pipe.child = Some(child);
        pipe.pump = Some(pump);
        pipe.failed = false;
        pipe
    }

    /// Reports whether there is no unacknowledged frame.
    pub fn idle(&self) -> bool {
        !self.pending
    }

    #[doc(hidden)]
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().map(OwnedChild::process_id)
    }

    /// Queues one bounded nonempty line without waiting.
    pub fn put(&mut self, bytes: Vec<u8>) -> bool {
        if self.failed || self.pending || bytes.is_empty() || bytes.len() > LINE_BYTES {
            return false;
        }
        match self
            .frames
            .as_ref()
            .expect("active transport sender")
            .try_send(bytes)
        {
            Ok(()) => {
                self.pending = true;
                true
            }
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => {
                self.failed = true;
                false
            }
        }
    }

    /// Returns one delivery result without waiting, or `None` while pending.
    pub fn pump(&mut self) -> Option<bool> {
        if !self.pending {
            return None;
        }
        match self.results.try_recv() {
            Ok(delivered) => {
                self.pending = false;
                if !delivered {
                    self.failed = true;
                    self.frames = None;
                }
                Some(delivered)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.pending = false;
                self.failed = true;
                self.frames = None;
                Some(false)
            }
        }
    }

    /// Stops accepting frames and reports whether one was discarded.
    pub fn discard(&mut self) -> bool {
        let discarded = self.pending;
        self.pending = false;
        self.failed = true;
        self.frames = None;
        discarded
    }

    /// Terminates the child if needed and joins the blocking pump by `deadline`.
    pub fn stop(&mut self, deadline: Instant) -> bool {
        let deadline = self
            .stop_deadline
            .map_or(deadline, |previous| previous.min(deadline));
        self.stop_deadline = Some(deadline);
        self.frames = None;
        let reaped = self
            .child
            .as_mut()
            .is_none_or(|child| child.reap_until(deadline).is_ok());
        while self
            .pump
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        let joined = match self.pump.take() {
            None => true,
            Some(thread) if thread.is_finished() => thread.join().is_ok(),
            Some(thread) => {
                self.pump = Some(thread);
                false
            }
        };
        reaped && joined
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        let deadline = self
            .stop_deadline
            .unwrap_or_else(|| Instant::now() + crate::limits::SHUTDOWN_TIME);
        if !self.stop(deadline) {
            std::process::abort();
        }
    }
}

fn pump(
    frames: Receiver<Vec<u8>>,
    delivered: SyncSender<bool>,
    mut writer: ChildStdin,
    mut ack: ChildStderr,
) {
    while let Ok(bytes) = frames.recv() {
        let length = (bytes.len() as u32).to_be_bytes();
        let result = writer
            .write_all(&length)
            .and_then(|()| writer.write_all(&bytes))
            .and_then(|()| writer.flush())
            .and_then(|()| {
                let mut ack_byte = [0u8; 1];
                ack.read_exact(&mut ack_byte).map(|()| ack_byte[0] == b'1')
            });
        let success = result.unwrap_or(false);
        if delivered.send(success).is_err() || !success {
            return;
        }
    }
}
