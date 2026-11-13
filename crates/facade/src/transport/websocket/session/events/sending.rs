//! Payload-free current-frame progress, including cancellation after start_send.

use std::task::{Context, Poll};
use std::time::Duration;

use super::super::WebSocketWireFrame;
use super::super::release::WebSocketWireRelease;
use crate::subscription::outbox::{
    RetiredSendProgress, SubscriptionOutboxSignal, SubscriptionSendProgress,
};

enum Progress {
    Normal(SubscriptionSendProgress),
    Retired(RetiredSendProgress),
}

pub(super) struct EventSend {
    progress: Option<Progress>,
    remaining: usize,
    flushed: bool,
    release: WebSocketWireRelease,
}

impl EventSend {
    pub(super) fn new(frame: &WebSocketWireFrame, progress: SubscriptionSendProgress) -> Self {
        Self {
            progress: Some(Progress::Normal(progress)),
            remaining: frame.as_bytes().len(),
            flushed: false,
            release: frame.release(),
        }
    }

    pub(super) fn accept(
        &mut self,
        accepted: usize,
        flushed: bool,
        now: u64,
    ) -> Result<(), SubscriptionOutboxSignal> {
        if self.flushed || accepted > self.remaining || (accepted == 0 && !flushed) {
            return Err(SubscriptionOutboxSignal::Invariant);
        }
        match self
            .progress
            .as_mut()
            .ok_or(SubscriptionOutboxSignal::Invariant)?
        {
            Progress::Normal(progress) => {
                progress.accept(accepted, now)?;
            }
            Progress::Retired(progress) => progress.accept(accepted, now)?,
        }
        self.remaining -= accepted;
        if flushed && self.remaining != 0 {
            return Err(SubscriptionOutboxSignal::Invariant);
        }
        self.flushed = flushed;
        Ok(())
    }

    pub(super) fn retire(&mut self, timeout: Duration) {
        self.progress = self.progress.take().map(|progress| match progress {
            Progress::Normal(progress) => Progress::Retired(progress.retire(timeout)),
            retired @ Progress::Retired(_) => retired,
        });
    }

    pub(super) fn poll_released(
        &self,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), SubscriptionOutboxSignal>> {
        if !self.flushed {
            return Poll::Pending;
        }
        self.release
            .poll(context)
            .map(|result| result.map_err(|_| SubscriptionOutboxSignal::Invariant))
    }

    pub(super) fn finish(mut self) -> Option<SubscriptionSendProgress> {
        match self.progress.take() {
            Some(Progress::Normal(progress)) => Some(progress),
            Some(Progress::Retired(_)) | None => None,
        }
    }

    pub(super) fn next_deadline(&self) -> Result<Option<u64>, SubscriptionOutboxSignal> {
        match &self.progress {
            Some(Progress::Retired(progress)) => progress.next_deadline(),
            _ => Err(SubscriptionOutboxSignal::Invariant),
        }
    }

    pub(super) fn on_timer(&self, now: u64) -> Result<(), SubscriptionOutboxSignal> {
        match &self.progress {
            Some(Progress::Retired(progress)) => progress.on_timer(now),
            _ => Err(SubscriptionOutboxSignal::Invariant),
        }
    }
}
