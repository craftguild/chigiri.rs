//! Fixed Luau member names for SDL-type source selection and publication.

/// Namespace containing phase-specific subscription source and publication functions.
pub(crate) const PUBSUB: &str = "pubsub";
/// Field-specific source selection functions.
pub(crate) const SUBSCRIBE: &str = "subscribe";
/// Namespace containing host-owned event generators during source setup.
pub(crate) const GENERATOR: &str = "generator";
/// Field-specific finite or looping sequence factory.
pub(crate) const SEQUENCE: &str = "sequence";
/// Dense array of complete field outputs captured at setup.
pub(crate) const VALUES: &str = "values";
/// Positive monotonic sequence interval in milliseconds.
pub(crate) const INTERVAL_MS: &str = "interval_ms";
/// Optional sequence repetition, false by default.
pub(crate) const LOOP: &str = "loop";
/// Inclusive maximum number of complete outputs in one definition.
pub(crate) const MAX_SEQUENCE_VALUES: usize = 1_000;
/// Inclusive maximum monotonic interval, one day in milliseconds.
pub(crate) const MAX_INTERVAL_MS: u64 = 86_400_000;
/// Binding and publication topic member.
pub(crate) const TOPIC: &str = "topic";
/// Optional server-side overflow selection.
pub(crate) const OVERFLOW: &str = "overflow";
/// Default non-dropping overflow behavior.
pub(crate) const BACKPRESSURE: &str = "backpressure";
/// Explicit discard-oldest overflow behavior.
pub(crate) const DROP_OLDEST: &str = "drop_oldest";
/// Field-specific publication functions in a write candidate.
pub(crate) const PUBLISH: &str = "publish";
/// Typed publication payload member.
pub(crate) const PAYLOAD: &str = "payload";
/// Dedicated static setup scope.
pub(crate) const SETUP_SCOPE: &str = "subscription_setup";
