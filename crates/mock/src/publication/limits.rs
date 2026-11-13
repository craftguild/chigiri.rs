//! Checked request publication settings and frozen host staging ceilings.

#[cfg(test)]
use super::PublicationError;

pub(super) const DEFAULT_PUBLICATIONS: usize = 32;
pub(super) const DEFAULT_CAPTURE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_PUBLICATIONS: usize = 1024;
#[cfg(test)]
pub(super) const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const STAGING_BATCHES: usize = 32;
pub(super) const STAGING_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_ACTIVE_SUBSCRIPTIONS: usize = 1024;
pub(super) const MAX_TOPIC_BYTES: usize = 256;
pub(crate) const MAX_SAFE_SEQUENCE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PublicationLimits {
    count: usize,
    bytes: usize,
}

impl PublicationLimits {
    #[cfg(test)]
    pub(super) fn new(count: usize, bytes: usize) -> Result<Self, PublicationError> {
        if count == 0 || count > MAX_PUBLICATIONS || bytes == 0 || bytes > MAX_CAPTURE_BYTES {
            return Err(PublicationError::InvalidConfiguration);
        }
        Ok(Self { count, bytes })
    }

    pub(super) const fn count(self) -> usize {
        self.count
    }

    pub(super) const fn bytes(self) -> usize {
        self.bytes
    }
}

impl Default for PublicationLimits {
    fn default() -> Self {
        Self {
            count: DEFAULT_PUBLICATIONS,
            bytes: DEFAULT_CAPTURE_BYTES,
        }
    }
}
