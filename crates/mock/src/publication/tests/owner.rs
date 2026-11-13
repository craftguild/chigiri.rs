//! Schema and dispatch owners bind capture before growth, reservation, or commit.

use super::super::{PublicationError, PublicationLimits, PublicationStore};
use super::support::{fixture, object_output, scalar};
use crate::store::{CallScope, Store};
use crate::value::ValueBudget;
use chigiri::core::{Object, Value};

#[test]
fn exact_ir_ownership_rejects_foreign_scalars_and_objects_before_capture_growth() {
    let first = fixture();
    let foreign = fixture(); // Same SDL is still a distinct authoritative IR owner.
    let store = PublicationStore::new(Store::empty(first.ir.clone()), 2).unwrap();
    for (field, output) in [
        (foreign.event, scalar(7_i64)),
        (foreign.dto, object_output(&foreign, foreign.dto, false)),
    ] {
        let buffer = store
            .capture_owner()
            .buffer(PublicationLimits::default())
            .unwrap();
        assert_eq!(
            buffer.push(&foreign.ir, field, "topic", &output),
            Err(PublicationError::ForeignOwner)
        );
        assert_eq!(
            buffer.prepare(&store.staging).err(),
            Some(PublicationError::InvalidPayload)
        );
        assert_eq!(store.staging.usage(), (0, 0));
        assert!(store.staging.take().unwrap().is_none());
    }
}

#[test]
fn a_capture_cannot_acquire_another_servers_staging_tokens_even_for_the_same_ir() {
    let fixture = fixture();
    let first = PublicationStore::new(Store::empty(fixture.ir.clone()), 2).unwrap();
    let second = PublicationStore::new(Store::empty(fixture.ir.clone()), 2).unwrap();
    for (field, output) in [
        (fixture.event, scalar(7_i64)),
        (fixture.dto, object_output(&fixture, fixture.dto, false)),
    ] {
        let buffer = first
            .capture_owner()
            .buffer(PublicationLimits::default())
            .unwrap();
        buffer.push(&fixture.ir, field, "topic", &output).unwrap();
        assert_eq!(
            buffer.prepare(&second.staging).err(),
            Some(PublicationError::ForeignOwner)
        );
        assert_eq!(first.staging.usage(), (0, 0));
        assert_eq!(second.staging.usage(), (0, 0));
        assert!(second.staging.take().unwrap().is_none());
    }
}

#[test]
fn a_sealed_foreign_batch_is_rejected_before_store_exchange_or_sequence_consumption() {
    let first = fixture();
    let second = fixture();
    let source = PublicationStore::new(Store::empty(first.ir.clone()), 2).unwrap();
    let mut target = PublicationStore::new(Store::empty(second.ir.clone()), 2).unwrap();
    let mut candidate = target.begin_request(ValueBudget::default());
    let mut root = candidate
        .store_mut()
        .begin(CallScope::MutationRoot)
        .unwrap();
    let row = Object::from_iter([
        ("id", Value::from("must-not-commit")),
        ("name", Value::from("foreign-capture")),
    ]);
    root.create(second.unit, &row).unwrap();
    candidate
        .store_mut()
        .commit(root.prepare().unwrap())
        .unwrap();
    let prepared_store = candidate.prepare().unwrap();
    let before = target.snapshot();
    let buffer = source
        .capture_owner()
        .buffer(PublicationLimits::default())
        .unwrap();
    buffer
        .push(&first.ir, first.event, "topic", &scalar(7_i64))
        .unwrap();
    let mut batch = buffer.prepare(&source.staging).unwrap();
    assert!(
        target
            .commit_publications(&prepared_store, &mut batch)
            .is_err()
    );
    assert!(batch.is_some());
    assert!(target.snapshot().same_version(&before));
    assert_eq!(target.snapshot().size().0, 0);
    assert!(target.staging.take().unwrap().is_none());
    assert_eq!(target.topics.commit_sequence().unwrap().value(), 1);
    assert_eq!(source.staging.usage().0, 1);
    drop(batch);
    assert_eq!(source.staging.usage(), (0, 0));
}
