//! Closed failures for private publication construction and accounting.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublicationError {
    InvalidConfiguration,
    InvalidTopic,
    InvalidCoordinate,
    InvalidPayload,
    DepthExceeded,
    CaptureLimit,
    PublicationCount,
    ArithmeticOverflow,
    Allocation,
    StagingCapacity,
    StagingClosed,
    ForeignOwner,
    SequenceExhausted,
    BindingCapacity,
    #[cfg(test)]
    UnknownBinding,
    #[cfg(test)]
    DispatcherBusy,
    Invariant,
}
