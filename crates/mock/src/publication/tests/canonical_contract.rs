//! Literal fixtures for ADR-0054's canonical byte table, not a second encoder.

use std::sync::Arc;

use chigiri::core::{Object, Source, Value};

use super::super::batch::BatchBuilder;
use super::super::{PublicationError, PublicationLimits};
use super::support::{fixture, object_output, record_output};
use crate::ir::{FieldId, MockIr};
use crate::output::OutputValue;
use crate::store::Store;

fn boundary(ir: &MockIr, field: FieldId, payload: &OutputValue, expected: usize) {
    let mut exact = BatchBuilder::new(PublicationLimits::new(2, expected).unwrap()).unwrap();
    exact.push(ir, field, "t", payload).unwrap();
    assert_eq!(exact.bytes(), expected);
    let mut below = BatchBuilder::new(PublicationLimits::new(2, expected - 1).unwrap()).unwrap();
    assert_eq!(
        below.push(ir, field, "t", payload),
        Err(PublicationError::CaptureLimit)
    );
    assert_eq!((below.len(), below.bytes()), (0, 0));
}

#[test]
fn literal_scalar_dto_stored_and_record_totals_match_the_frozen_byte_table() {
    let fixture = fixture();
    // Batch tag/length 9 + publication/batch-entry frames 32 + topic 1
    // + Subscription.event 18 + Null tag/payload 2. No schema digest is retained.
    boundary(&fixture.ir, fixture.event, &OutputValue::Null, 62);
    let dto = object_output(&fixture, fixture.dto, false);
    // Envelope tag 1 + type name 8 + field length 8 + field frame 16
    // + field name 5 + String tag/value 5 = 43; outer fixed bytes are 58.
    boundary(&fixture.ir, fixture.dto, &dto, 101);
    let stored = object_output(&fixture, fixture.stored, true);
    boundary(&fixture.ir, fixture.stored, &stored, 104);
    let (_store, record) = record_output(&fixture, "original");
    // Unit: tag/type/length 13 + three field frames 48 + field names 10
    // + values (ID 2, name 9, null 2) + Record digest/model/public-ID 37 = 121.
    boundary(&fixture.ir, fixture.changed, &record, 183);

    let mut duplicate = BatchBuilder::new(PublicationLimits::new(2, 115).unwrap()).unwrap();
    duplicate
        .push(&fixture.ir, fixture.event, "t", &OutputValue::Null)
        .unwrap();
    duplicate
        .push(&fixture.ir, fixture.event, "t", &OutputValue::Null)
        .unwrap();
    assert_eq!(duplicate.bytes(), 115); // 62 + the second 53-byte publication.
}

#[test]
fn literal_empty_and_nested_typed_objects_each_have_one_value_tag() {
    let ir = Arc::new(
        MockIr::parse(vec![Source::new(
            "canonical.graphql",
            r#"
        type Query { ping: Int }
        type Subscription { nested: Outer! empty: Empty! }
        type Outer { inner: Inner! }
        type Inner { value: String! }
        type Empty { computed: String @virtual }
    "#,
        )])
        .unwrap(),
    );
    let root = ir.graph().subscription().unwrap();
    let nested = ir.graph().types()[root.0].fields["nested"];
    let empty = ir.graph().types()[root.0].fields["empty"];
    let snapshot = Store::empty(ir.clone()).snapshot();
    let inner = Object::from_iter([("value", Value::from("x"))]);
    let outer = Object::from_iter([("inner", Value::Object(inner))]);
    let nested_output = snapshot
        .reader()
        .output(nested, &Value::Object(outer))
        .unwrap();
    // Inner = 1+5+8+16+5+2 = 37; Outer = 1+5+8+16+5+37 = 72.
    // Batch/publication/topic/coordinate = 9+32+1+19 = 61.
    boundary(&ir, nested, &nested_output, 133);
    let empty_output = snapshot
        .reader()
        .output(empty, &Value::Object(Object::new()))
        .unwrap();
    // Empty = tag 1 + type 5 + collection length 8; fixed outer bytes are 60.
    boundary(&ir, empty, &empty_output, 74);
}
