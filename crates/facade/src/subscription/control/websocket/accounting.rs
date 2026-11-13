//! One unchanged fixed quota for every queued WebSocket control class.

use super::SubscriptionControlSignal;

pub(in crate::subscription::control) const MAX_QUEUED_CONTROL_MESSAGES: usize = 32;
pub(in crate::subscription::control) const MAX_QUEUED_CONTROL_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::subscription::control) struct QueueAccounting {
    pub(in crate::subscription::control) messages: usize,
    pub(in crate::subscription::control) bytes: usize,
}

impl QueueAccounting {
    pub(in crate::subscription::control) fn exact_candidate(
        self,
        candidate_bytes: usize,
    ) -> Result<(), SubscriptionControlSignal> {
        self.check(candidate_bytes).map(|_| ())
    }

    pub(in crate::subscription::control) fn dynamic_candidate(
        self,
    ) -> Result<usize, SubscriptionControlSignal> {
        self.check(0)?;
        let remaining = MAX_QUEUED_CONTROL_BYTES
            .checked_sub(self.bytes)
            .ok_or(SubscriptionControlSignal::Accounting)?;
        if remaining == 0 {
            Err(SubscriptionControlSignal::QueueCapacity)
        } else {
            Ok(remaining)
        }
    }

    pub(in crate::subscription::control) fn admit(
        &mut self,
        candidate_bytes: usize,
    ) -> Result<(), SubscriptionControlSignal> {
        let (messages, bytes) = self.check(candidate_bytes)?;
        self.messages = messages;
        self.bytes = bytes;
        Ok(())
    }

    fn check(self, candidate_bytes: usize) -> Result<(usize, usize), SubscriptionControlSignal> {
        let messages = self
            .messages
            .checked_add(1)
            .ok_or(SubscriptionControlSignal::Accounting)?;
        let bytes = self
            .bytes
            .checked_add(candidate_bytes)
            .ok_or(SubscriptionControlSignal::Accounting)?;
        if messages > MAX_QUEUED_CONTROL_MESSAGES || bytes > MAX_QUEUED_CONTROL_BYTES {
            Err(SubscriptionControlSignal::QueueCapacity)
        } else {
            Ok((messages, bytes))
        }
    }

    pub(super) fn remove(&mut self, bytes: usize) -> Result<(), SubscriptionControlSignal> {
        self.messages = self
            .messages
            .checked_sub(1)
            .ok_or(SubscriptionControlSignal::Accounting)?;
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or(SubscriptionControlSignal::Accounting)?;
        Ok(())
    }
}
