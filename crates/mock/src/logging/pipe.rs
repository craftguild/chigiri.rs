//! Attribute unconfirmed lines to their original independently reserved log class.

use std::path::Path;
use std::process::Stdio;
use std::time::Instant;

use super::{Counters, Kind, Record};

pub(super) struct Pipe {
    output: crate::stdio_writer::Pipe,
    pending: Option<Kind>,
}

impl Pipe {
    pub fn start(executable: &Path, output: std::io::Result<Stdio>) -> Self {
        Self {
            output: crate::stdio_writer::Pipe::start(executable, output),
            pending: None,
        }
    }
    pub fn idle(&self) -> bool {
        self.pending.is_none()
    }
    pub fn put(&mut self, record: Record, counts: &Counters) {
        assert!(
            self.idle(),
            "each sink permits only one unacknowledged frame"
        );
        if self.output.put(record.bytes) {
            self.pending = Some(record.kind);
        } else {
            counts.lose(record.kind);
        }
    }
    pub fn pump(&mut self, counts: &Counters) {
        if let Some(delivered) = self.output.pump() {
            let kind = self
                .pending
                .take()
                .expect("an acknowledged frame must retain its record kind");
            if !delivered {
                counts.lose(kind);
            }
        }
    }
    pub fn discard(&mut self, counts: &Counters) {
        if let Some(kind) = self.pending.take() {
            counts.lose(kind);
            self.output.discard();
        }
    }
    pub fn stop(&mut self, deadline: Instant) -> bool {
        self.output.stop(deadline)
    }
}
