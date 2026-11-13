//! Fallible retained-metadata limits with the accepted default and hard ceiling.

use std::fmt;

use crate::subscription::settings::SubscriptionLimits;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MetadataRetentionLimit(usize);

impl MetadataRetentionLimit {
    pub(crate) fn from_validated(
        settings: &crate::subscription::settings::ValidatedSubscriptionSettings,
    ) -> Self {
        Self(settings.limits().max_retained_metadata_bytes)
    }

    pub(crate) fn new(bytes: usize) -> Result<Self, RetentionError> {
        if !(SubscriptionLimits::MIN_BYTES..=SubscriptionLimits::MAX_RETAINED_METADATA_BYTES)
            .contains(&bytes)
        {
            return Err(RetentionError::InvalidLimit);
        }
        Ok(Self(bytes))
    }

    pub(crate) fn check(self, bytes: usize) -> Result<(), RetentionError> {
        if bytes > self.0 {
            Err(RetentionError::Exceeded)
        } else {
            Ok(())
        }
    }

    pub(crate) fn available(self, bytes: usize) -> Result<usize, RetentionError> {
        self.0.checked_sub(bytes).ok_or(RetentionError::Exceeded)
    }
}

impl Default for MetadataRetentionLimit {
    fn default() -> Self {
        Self(
            crate::subscription::settings::SubscriptionLimits::default()
                .max_retained_metadata_bytes,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetentionError {
    InvalidLimit,
    Overflow,
    Exceeded,
    InitializationForHttp,
}

impl fmt::Display for RetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("subscriber metadata cannot be retained within its bounded contract")
    }
}

impl std::error::Error for RetentionError {}
