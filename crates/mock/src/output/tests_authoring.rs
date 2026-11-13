//! ADR-0047 output diagnostic classifications and sanitized schema details.

use std::sync::Arc;

use chigiri::core::{Source, Value};

use super::tests::{field, store, value};
use crate::ir::MockIr;
use crate::store::Store;

#[test]
fn ab_09_output_failures_keep_safe_classes_and_schema_members() {
    let public = vec![Source::new(
        "schema.graphql",
        "type Image{url:String!} type Query{image:Image! count:Int!}",
    )];
    let storage = vec![Source::new(
        "storage.graphql",
        "extend type Image{ownerId:ID!}",
    )];
    let ir = Arc::new(MockIr::parse_with_storage(public, storage).unwrap());
    let snapshot = Store::empty(ir.clone()).snapshot();
    let image = field(&ir, "Query", "image");
    let private = super::validate_detailed(
        &snapshot,
        image,
        &value(r#"{"url":"safe","ownerId":"not-logged"}"#),
        &mut Default::default(),
    )
    .err()
    .unwrap()
    .diagnostic(&ir, image);
    assert_eq!(private.cause, "MOCK_OUTPUT_PRIVATE_FIELD");
    assert_eq!(private.member, "Image.ownerId");
    assert_eq!(private.actual, "private field supplied by resolver DTO");
    assert!(!private.expected.contains("not-logged"));

    let required =
        super::validate_detailed(&snapshot, image, &value("{}"), &mut Default::default())
            .err()
            .unwrap()
            .diagnostic(&ir, image);
    assert_eq!(required.cause, "MOCK_OUTPUT_REQUIRED_FIELD");
    assert_eq!(required.member, "Image.url");
    assert!(required.expected.contains("String!"));

    let stored =
        super::validate_stored_detailed(&snapshot, image, &value("{}"), &mut Default::default())
            .err()
            .unwrap()
            .diagnostic(&ir, image);
    assert_eq!(stored.cause, "MOCK_STORED_VALUE_SHAPE");
    assert_eq!(stored.member, "Image.url");

    let count = field(&ir, "Query", "count");
    let mismatch = super::validate_detailed(
        &snapshot,
        count,
        &Value::from("private-actual"),
        &mut Default::default(),
    )
    .err()
    .unwrap()
    .diagnostic(&ir, count);
    assert_eq!(mismatch.cause, "MOCK_OUTPUT_TYPE_MISMATCH");
    assert_eq!(mismatch.member, "Query.count");
    assert!(mismatch.expected.contains("Int!"));
    assert!(!mismatch.actual.contains("private-actual"));

    let snapshot = store().snapshot();
    let unit = field(snapshot.ir(), "Query", "unit");
    let provenance = super::validate_detailed(
        &snapshot,
        unit,
        &value(r#"{"id":"forged","name":"copy"}"#),
        &mut Default::default(),
    )
    .err()
    .unwrap()
    .diagnostic(snapshot.ir(), unit);
    assert_eq!(provenance.cause, "MOCK_RECORD_PROVENANCE");
    assert_eq!(provenance.member, "Query.unit");
}
