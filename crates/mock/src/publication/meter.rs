//! Checked accounting performed before each owned canonical allocation.

use super::PublicationError;

pub(super) const TAG_BYTES: usize = 1;
pub(super) const PAYLOAD_BYTE: usize = 1;
pub(super) const SCALAR_BYTES: usize = 8;
pub(super) const LENGTH_BYTES: usize = 8;
pub(super) const ENTRY_BYTES: usize = 16;
pub(super) const DIGEST_BYTES: usize = 32;

#[derive(Clone, Copy, Debug)]
pub(super) struct Meter {
    used: usize,
    limit: usize,
}

impl Meter {
    pub(super) const fn new(used: usize, limit: usize) -> Self {
        Self { used, limit }
    }

    pub(super) fn add(&mut self, bytes: usize) -> Result<(), PublicationError> {
        let next = self
            .used
            .checked_add(bytes)
            .ok_or(PublicationError::ArithmeticOverflow)?;
        if next > self.limit {
            return Err(PublicationError::CaptureLimit);
        }
        self.used = next;
        Ok(())
    }

    pub(super) fn collection(&mut self, entries: usize) -> Result<(), PublicationError> {
        let bytes = field_bytes(entries)?
            .checked_add(TAG_BYTES)
            .ok_or(PublicationError::ArithmeticOverflow)?;
        self.add(bytes)
    }

    /// Typed objects have already charged their one value tag.
    pub(super) fn fields(&mut self, entries: usize) -> Result<(), PublicationError> {
        self.add(field_bytes(entries)?)
    }

    pub(super) const fn used(self) -> usize {
        self.used
    }
}

fn field_bytes(entries: usize) -> Result<usize, PublicationError> {
    let entry_bytes = entries
        .checked_mul(ENTRY_BYTES)
        .ok_or(PublicationError::ArithmeticOverflow)?;
    LENGTH_BYTES
        .checked_add(entry_bytes)
        .ok_or(PublicationError::ArithmeticOverflow)
}
