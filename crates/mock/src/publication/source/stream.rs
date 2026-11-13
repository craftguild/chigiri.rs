//! Expose a move-only stream while retaining its exact registration identity.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use super::super::retained::CapturedEvent;
use super::super::topics::RegisteredBinding;
use crate::luau::ScriptError;

pub(crate) struct PublicationSource {
    binding: RegisteredBinding,
}

pub(crate) struct RegisteredSubscription {
    source: PublicationSource,
    overflow: chigiri::SubscriptionOverflow,
}

impl PublicationSource {
    pub(super) const fn new(binding: RegisteredBinding) -> Self {
        Self { binding }
    }
}

impl RegisteredSubscription {
    pub(crate) fn activate(&self) {
        self.source.binding.port().activate();
    }

    #[cfg(test)]
    pub(in crate::publication) fn activate_at(&self, now: std::time::Instant) {
        self.source.binding.port().activate_at(now);
    }

    pub(crate) fn poll(
        &mut self,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<CapturedEvent, ScriptError>>> {
        Pin::new(&mut self.source).poll_next(context)
    }

    pub(crate) fn source_delivery(&self) -> std::sync::Arc<chigiri::__private::HostSourceDelivery> {
        self.source.binding.port().source_delivery()
    }
    pub(in crate::publication) fn new(
        binding: RegisteredBinding,
        overflow: chigiri::SubscriptionOverflow,
    ) -> Self {
        Self {
            source: PublicationSource::new(binding),
            overflow,
        }
    }

    pub(crate) fn into_parts(self) -> (PublicationSource, chigiri::SubscriptionOverflow) {
        (self.source, self.overflow)
    }
}

impl Stream for PublicationSource {
    type Item = Result<CapturedEvent, ScriptError>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.binding.port().poll(context)
    }
}

impl Drop for PublicationSource {
    fn drop(&mut self) {
        self.binding.port().close();
    }
}
