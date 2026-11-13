//! Exact canonical accounting and sealed-capture proofs.

use chigiri::core::{Object, Value};

use super::super::batch::BatchBuilder;
use super::super::capture::{CapturedOrigin, CapturedValue};
use super::super::limits::{MAX_CAPTURE_BYTES, MAX_PUBLICATIONS};
use super::super::meter::Meter;
use super::super::{PublicationError, PublicationLimits};
use super::support::{exact_limits, fixture, object_output, record_output, replace_record, scalar};
use crate::output::OutputValue;

const BATCH: usize = 1 + 8;
const PUBLICATION: usize = 16 + 16;

fn fixed(topic: &str, coordinate: &str) -> usize {
    BATCH + PUBLICATION + topic.len() + coordinate.len()
}

fn measured(payload: &OutputValue) -> usize {
    let fixture = fixture();
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push(&fixture.ir, fixture.event, "t", payload)
        .unwrap();
    builder.bytes()
}

#[test]
fn exact_scalar_and_nested_collection_charges_have_one_over_rejection() {
    let mut map = Object::new();
    map.insert("a", Value::Null);
    let cases = [
        (OutputValue::Null, 2),
        (scalar(true), 2),
        (scalar(7_i64), 1 + 8),
        (scalar(Value::Float(1.5)), 1 + 8),
        (scalar(Value::BigInt("12345".into())), 1 + 5),
        (scalar("same"), 1 + 4),
        (
            scalar(Value::List(vec![Value::from("x"), Value::from("x")])),
            1 + 8 + 2 * 16 + 2 * (1 + 1),
        ),
        (scalar(Value::Object(map)), 1 + 8 + 16 + 1 + 2),
    ];
    for (payload, payload_bytes) in cases {
        let fixture = fixture();
        let expected = fixed("t", "Subscription.event") + payload_bytes;
        assert_eq!(measured(&payload), expected);
        let mut exact = BatchBuilder::new(exact_limits(expected)).unwrap();
        exact
            .push(&fixture.ir, fixture.event, "t", &payload)
            .unwrap();
        assert_eq!(exact.bytes(), expected);
        let mut below = BatchBuilder::new(exact_limits(expected - 1)).unwrap();
        assert_eq!(
            below.push(&fixture.ir, fixture.event, "t", &payload),
            Err(PublicationError::CaptureLimit)
        );
        assert_eq!((below.len(), below.bytes()), (0, 0));
    }
}

#[test]
fn dto_stored_and_record_objects_charge_each_retained_tag_and_provenance_byte() {
    let fixture = fixture();
    for (field, coordinate, stored, origin) in [
        (fixture.dto, "Subscription.dto", false, CapturedOrigin::Dto),
        (
            fixture.stored,
            "Subscription.stored",
            true,
            CapturedOrigin::Stored,
        ),
    ] {
        let output = object_output(&fixture, field, stored);
        assert_object_exact(&fixture, field, coordinate, &output, &origin);
    }

    let (_store, output) = record_output(&fixture, "original");
    let mut measured = BatchBuilder::new(PublicationLimits::default()).unwrap();
    measured
        .push(&fixture.ir, fixture.changed, "t", &output)
        .unwrap();
    let measured = measured.seal().unwrap();
    let publication = &measured.publications()[0];
    let expected = fixed("t", "Subscription.changed") + charge(publication.payload());
    let mut exact = BatchBuilder::new(exact_limits(expected)).unwrap();
    exact
        .push(&fixture.ir, fixture.changed, "t", &output)
        .unwrap();
    assert_eq!(exact.bytes(), expected);
    let mut below = BatchBuilder::new(exact_limits(expected - 1)).unwrap();
    assert_eq!(
        below.push(&fixture.ir, fixture.changed, "t", &output),
        Err(PublicationError::CaptureLimit)
    );
    assert_eq!((below.len(), below.bytes()), (0, 0));
}

fn assert_object_exact(
    fixture: &super::support::Fixture,
    field: crate::ir::FieldId,
    coordinate: &str,
    output: &OutputValue,
    origin: &CapturedOrigin,
) {
    let mut measured = BatchBuilder::new(PublicationLimits::default()).unwrap();
    measured.push(&fixture.ir, field, "t", output).unwrap();
    let batch = measured.seal().unwrap();
    let payload = batch.publications()[0].payload();
    let CapturedValue::Object(object) = payload else {
        panic!("object capture required")
    };
    assert_eq!(&object.origin, origin);
    let expected = fixed("t", coordinate) + charge(payload);
    assert_eq!(batch.bytes(), expected);
    let mut below = BatchBuilder::new(exact_limits(expected - 1)).unwrap();
    assert_eq!(
        below.push(&fixture.ir, field, "t", output),
        Err(PublicationError::CaptureLimit)
    );
    assert_eq!((below.len(), below.bytes()), (0, 0));
}

fn charge(value: &CapturedValue) -> usize {
    match value {
        CapturedValue::Null | CapturedValue::Boolean(_) => 2,
        CapturedValue::Integer(_) | CapturedValue::Float(_) => 1 + 8,
        CapturedValue::BigInt(value) | CapturedValue::String(value) => 1 + value.len(),
        CapturedValue::List(values) => {
            1 + 8 + 16 * values.len() + values.iter().map(charge).sum::<usize>()
        }
        CapturedValue::Map(fields) => field_charge(fields),
        CapturedValue::Object(object) => {
            let origin = match &object.origin {
                CapturedOrigin::Dto | CapturedOrigin::Stored => 0,
                CapturedOrigin::Record {
                    model,
                    public_identity,
                    ..
                } => 32 + model.len() + public_identity.as_deref().map_or(0, str::len),
            };
            object.type_name.len() + origin + field_charge(&object.fields)
        }
    }
}

fn field_charge(fields: &[super::super::capture::CapturedField]) -> usize {
    1 + 8
        + 16 * fields.len()
        + fields
            .iter()
            .map(|field| field.name.len() + charge(&field.value))
            .sum::<usize>()
}

#[test]
fn duplicate_publications_and_request_count_are_charged_before_mutation() {
    let fixture = fixture();
    let payload = scalar("duplicate");
    let first = measured(&payload);
    let repeated = PUBLICATION + 1 + "Subscription.event".len() + 1 + 9;
    let mut builder = BatchBuilder::new(exact_limits(first + repeated)).unwrap();
    builder
        .push(&fixture.ir, fixture.event, "t", &payload)
        .unwrap();
    builder
        .push(&fixture.ir, fixture.event, "t", &payload)
        .unwrap();
    assert_eq!(builder.bytes(), first + repeated);

    let limits = PublicationLimits::new(2, MAX_CAPTURE_BYTES).unwrap();
    let mut count = BatchBuilder::new(limits).unwrap();
    count
        .push(&fixture.ir, fixture.event, "a", &payload)
        .unwrap();
    count
        .push(&fixture.ir, fixture.event, "b", &payload)
        .unwrap();
    let before = count.bytes();
    assert_eq!(
        count.push(&fixture.ir, fixture.event, "c", &payload),
        Err(PublicationError::PublicationCount)
    );
    assert_eq!((count.len(), count.bytes()), (2, before));
}

#[test]
fn sealed_record_remains_old_after_delete_and_identity_recreation() {
    let fixture = fixture();
    let (mut store, output) = record_output(&fixture, "original");
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    builder
        .push(&fixture.ir, fixture.changed, "record", &output)
        .unwrap();
    let captured_bytes = builder.bytes();
    let batch = builder.seal().unwrap();
    drop(output);
    replace_record(&mut store, &fixture);
    drop(store);

    let publication = &batch.publications()[0];
    let CapturedValue::Object(object) = publication.payload() else {
        panic!("record capture required")
    };
    let CapturedOrigin::Record {
        model,
        public_identity,
        digest,
    } = &object.origin
    else {
        panic!("record provenance required")
    };
    assert_eq!(
        (model.as_ref(), public_identity.as_deref()),
        ("Unit", Some("u"))
    );
    assert_ne!(*digest, [0; 32]);
    let original_digest = *digest;
    let name = object
        .fields
        .iter()
        .find(|field| field.name.as_ref() == "name")
        .unwrap();
    assert_eq!(name.value, CapturedValue::String("original".into()));
    assert_eq!(batch.bytes(), captured_bytes);

    let (_replacement_store, replacement) = record_output(&fixture, "replacement");
    let mut replacement_builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    replacement_builder
        .push(&fixture.ir, fixture.changed, "record", &replacement)
        .unwrap();
    let replacement_batch = replacement_builder.seal().unwrap();
    let CapturedValue::Object(replacement_object) = replacement_batch.publications()[0].payload()
    else {
        panic!("replacement record capture required")
    };
    let CapturedOrigin::Record {
        digest: replacement_digest,
        ..
    } = &replacement_object.origin
    else {
        panic!("replacement record provenance required")
    };
    assert_ne!(original_digest, *replacement_digest);
}

#[test]
fn foreign_schema_object_fails_before_builder_state_changes() {
    let expected = fixture();
    let foreign = fixture();
    let output = object_output(&foreign, foreign.dto, false);
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    assert_eq!(
        builder.push(&expected.ir, expected.dto, "topic", &output),
        Err(PublicationError::InvalidPayload)
    );
    assert_eq!((builder.len(), builder.bytes()), (0, 0));
}

#[test]
fn invalid_float_fails_before_builder_state_changes() {
    let fixture = fixture();
    let mut builder = BatchBuilder::new(PublicationLimits::default()).unwrap();
    assert_eq!(
        builder.push(
            &fixture.ir,
            fixture.event,
            "topic",
            &scalar(Value::Float(f64::INFINITY)),
        ),
        Err(PublicationError::InvalidPayload)
    );
    assert_eq!((builder.len(), builder.bytes()), (0, 0));
}

#[test]
fn constructors_reject_zero_and_hard_max_one_over_without_clamping() {
    for invalid in [
        PublicationLimits::new(0, 1),
        PublicationLimits::new(MAX_PUBLICATIONS + 1, 1),
        PublicationLimits::new(1, 0),
        PublicationLimits::new(1, MAX_CAPTURE_BYTES + 1),
    ] {
        assert_eq!(invalid, Err(PublicationError::InvalidConfiguration));
    }
}

#[test]
fn checked_meter_overflow_never_partially_advances_the_trial() {
    let mut addition = Meter::new(usize::MAX, usize::MAX);
    assert_eq!(addition.add(1), Err(PublicationError::ArithmeticOverflow));
    assert_eq!(addition.used(), usize::MAX);

    let mut collection = Meter::new(7, usize::MAX);
    assert_eq!(
        collection.collection(usize::MAX),
        Err(PublicationError::ArithmeticOverflow)
    );
    assert_eq!(collection.used(), 7);
}
