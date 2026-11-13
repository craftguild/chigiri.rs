//! Inspect actual staging ownership in worker fixtures, never as a test oracle.

use chigiri::core::Value;

use super::{PublicationStore, RegisteredSubscription};
use crate::ir::{FieldId, MockIr};
use crate::output::OutputValue;

pub(crate) fn shares_capture(left: &super::CapturedEvent, right: &super::CapturedEvent) -> bool {
    std::ptr::eq(left.publication(), right.publication())
}

pub(crate) fn usage(store: &PublicationStore) -> (usize, usize) {
    store.staging.usage()
}

pub(crate) fn stage_idle(store: &PublicationStore) -> bool {
    store.staging.front().unwrap().is_none()
}

pub(crate) fn take_topics(store: &PublicationStore) -> Vec<Vec<String>> {
    let mut batches = Vec::new();
    while let Some(batch) = store.staging.take().unwrap() {
        batches.push(
            batch
                .batch()
                .publications()
                .iter()
                .map(|publication| publication.topic().to_owned())
                .collect(),
        );
    }
    batches
}

pub(crate) fn register_source(
    store: &mut PublicationStore,
    ir: &MockIr,
    field: FieldId,
    topic: &str,
) -> RegisteredSubscription {
    let prepared = store
        .registration_owner()
        .prepare(ir, field, topic)
        .unwrap();
    store
        .register_source(prepared, chigiri::SubscriptionOverflow::Backpressure)
        .ok()
        .unwrap()
        .finish()
}

pub(crate) fn stage_scalar(
    store: &mut PublicationStore,
    ir: &MockIr,
    field: FieldId,
    topic: &str,
    value: &str,
) {
    let staging = store.staging();
    let buffer = store
        .capture_owner()
        .buffer(super::PublicationLimits::default())
        .unwrap();
    buffer
        .push(ir, field, topic, &OutputValue::Scalar(Value::from(value)))
        .unwrap();
    let prepared = buffer.prepare(&staging).unwrap().unwrap();
    let sequence = store.topics.commit_sequence().unwrap();
    staging
        .stage_retained(prepared.retained, sequence)
        .ok()
        .unwrap();
}
