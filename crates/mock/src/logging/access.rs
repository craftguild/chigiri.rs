//! Distinguish cancellation before response generation from body completion, emitting one terminal record per request.

use std::time::Instant;

use super::{Kind, Logger, RECORD_BYTES, records};

/// Owner of one access record spanning request admission through response-body completion.
pub struct Access {
    logger: Logger,
    id: u64,
    metadata: Option<(String, String)>,
    start: Instant,
    status: Option<u16>,
    finished: bool,
}
impl Access {
    /// Capture method and path without query text; oversized metadata becomes one delivery loss.
    pub fn new(logger: Logger, id: u64, method: &str, path: &str) -> Self {
        let metadata = (method.len().saturating_add(path.len()) <= RECORD_BYTES)
            .then(|| (method.to_owned(), path.to_owned()));
        Self {
            logger,
            id,
            metadata,
            start: Instant::now(),
            status: None,
            finished: false,
        }
    }
    /// Record only the status actually produced, including a Mock override.
    pub fn response(&mut self, status: u16) {
        self.status = Some(status);
    }
    /// Mark response-body processing complete and emit the access record.
    pub fn complete(&mut self) {
        self.finish(true);
    }
    fn finish(&mut self, completed: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        let bytes = self
            .metadata
            .as_ref()
            .ok_or(crate::value::ValueError("access metadata limit"))
            .and_then(|(method, path)| {
                records::access(
                    self.id,
                    method,
                    path,
                    self.status,
                    self.start.elapsed(),
                    completed,
                )
            });
        self.logger.enqueue(Kind::Access, bytes);
    }
}
impl Drop for Access {
    fn drop(&mut self) {
        self.finish(false);
    }
}
