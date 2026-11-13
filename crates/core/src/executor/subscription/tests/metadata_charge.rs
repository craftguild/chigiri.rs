use std::sync::Arc;

use crate::executor::{ExecutableDirectiveSite, prepare_subscription_plan};
use crate::{
    DocumentDigest, ExecutionLimits, OperationIdentity, OperationKey, OperationName, SchemaDigest,
};

use super::fixture::Fixture;

#[test]
fn prepared_plan_charge_matches_the_complete_default_owned_inventory() {
    let fixture = Fixture::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let plan = prepare_subscription_plan(&operation, None, None, None, None).unwrap();
    let root = &plan.root;

    assert_eq!(operation.variables.iter().len(), 1);
    assert_eq!(root.locations.len(), 1);
    assert_eq!(root.arguments.len(), 1);
    assert_eq!(root.operation_directives.len(), 2);
    assert_eq!(root.directives.len(), 1);
    assert_eq!(root.cost_fields.len(), 3);
    assert!(matches!(
        root.operation_directives[0].site(),
        ExecutableDirectiveSite::Operation
    ));
    assert!(matches!(
        root.operation_directives[1].site(),
        ExecutableDirectiveSite::Field { field } if field == "event"
    ));
    assert!(matches!(
        root.directives[0].site(),
        ExecutableDirectiveSite::Field { field } if field == "event"
    ));
    assert_eq!(
        root.cost_fields
            .iter()
            .map(|field| field.coordinate())
            .collect::<Vec<_>>(),
        ["Subscription.event", "Event.required", "Event.label"]
    );

    let source = &operation.source;
    let source_bytes = source.name().len() + source.text().len() + 16 * source.line_starts().len();
    let variables = 16 + "id".len() + "room".len();
    let identity = 32 + 32;
    let root_names = "Subscription".len() + "event".len() + "chosen".len();
    let locations = 16;
    let public_arguments = 2 * 16 + 2 * "id".len() + "room".len();
    let dispatch_arguments = 16 + "room".len();
    let directive_value = "audit".len() + 2 * 16 + 2 * "level".len();
    let operation_directives = 2 * 16 + directive_value + (directive_value + "event".len());
    let field_directive = 16 + directive_value + "event".len();
    let cost_fields = (16 + "Subscription.event".len() + 16 + "id".len() + "ID!".len())
        + (16 + "Event.required".len())
        + (16 + "Event.label".len());
    let expected = source_bytes
        + variables
        + identity
        + root_names
        + locations
        + public_arguments
        + dispatch_arguments
        + operation_directives
        + field_directive
        + cost_fields;

    assert_eq!(plan.retained_metadata_bytes(), Some(expected));
}

#[test]
fn distinct_owned_operation_names_are_charged_while_shared_arc_controls_are_not() {
    let fixture = Fixture::new();
    let operation = fixture.operation(ExecutionLimits::unbounded());
    let baseline = prepare_subscription_plan(&operation, None, None, None, None)
        .unwrap()
        .retained_metadata_bytes()
        .unwrap();
    let mut named = operation.clone();
    named.operation_name = Some("Events".to_owned());
    named.identity = Arc::new(OperationIdentity::new(
        SchemaDigest::from_bytes([1; 32]),
        OperationKey::new(
            DocumentDigest::from_bytes([2; 32]),
            Some(OperationName::new("Events").unwrap()),
        ),
    ));
    let schema_owner = Arc::clone(&named.schema);
    let document_owner = Arc::clone(&named.document);
    let source_owner = Arc::clone(&named.source);
    let identity_owner = Arc::clone(&named.identity);

    let named_bytes = prepare_subscription_plan(&named, None, None, None, None)
        .unwrap()
        .retained_metadata_bytes()
        .unwrap();

    assert_eq!(named_bytes, baseline + 2 * "Events".len());
    assert!(Arc::strong_count(&schema_owner) > 1);
    assert!(Arc::strong_count(&document_owner) > 1);
    assert!(Arc::strong_count(&source_owner) > 1);
    assert!(Arc::strong_count(&identity_owner) > 1);
}
