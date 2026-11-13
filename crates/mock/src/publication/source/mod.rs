//! Own one bounded typed FIFO and cancellation token for each active source.

pub(in crate::publication) mod accounting;
pub(in crate::publication) mod admission;
mod port;
mod queue;
mod stream;

pub(super) use port::{Offer, SourcePort, Termination};
#[cfg(test)]
pub(super) use stream::PublicationSource;
pub(crate) use stream::RegisteredSubscription;
