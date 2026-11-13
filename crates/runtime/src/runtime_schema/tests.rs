//! Regression tests for constructing a semantic schema from generated descriptors.

use crate::{
    DescriptorList, FieldDescriptor, GENERATED_ABI_VERSION, OperationRootsDescriptor,
    SchemaDescriptor, TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor,
};

use super::build;

static STRING_TYPE: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static QUERY_FIELD: FieldDescriptor = FieldDescriptor {
    name: "hello",
    description: Some("greeting"),
    type_ref: "String",
    type_descriptor: TypeRefDescriptor::Named("String"),
    arguments: DescriptorList::empty(),
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static QUERY_FIELDS: &[FieldDescriptor] = &[QUERY_FIELD];
static QUERY_TYPE: TypeDescriptor = TypeDescriptor {
    name: "Query",
    kind: TypeDescriptorKind::Object,
    description: Some("root"),
    fields: DescriptorList::new(&[QUERY_FIELDS]),
    interfaces: DescriptorList::empty(),
    possible_types: DescriptorList::empty(),
    enum_values: DescriptorList::empty(),
    input_fields: DescriptorList::empty(),
    directives: DescriptorList::empty(),
    is_one_of: false,
    specified_by_url: None,
};
static TYPES: &[TypeDescriptor] = &[QUERY_TYPE];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: GENERATED_ABI_VERSION,
    schema_digest: crate::SchemaDigest::from_bytes([1; 32]),
    description: Some("runtime schema"),
    schema_directives: DescriptorList::empty(),
    roots: OperationRootsDescriptor {
        query: "Query",
        mutation: None,
        subscription: None,
    },
    types: DescriptorList::new(&[TYPES]),
    built_in_scalar_overlays: DescriptorList::empty(),
    directives: DescriptorList::empty(),
};

#[test]
fn converts_descriptors_to_semantic_indexes_without_reparsing() {
    let schema = build(&SCHEMA).unwrap();
    let query = schema.type_by_name("Query").unwrap();

    assert_eq!(schema.description().unwrap().value.value, "runtime schema");
    assert_eq!(query.description().unwrap().value.value, "root");
    assert_eq!(
        query
            .field("hello")
            .unwrap()
            .description
            .as_ref()
            .unwrap()
            .value
            .value,
        "greeting"
    );
    assert_eq!(STRING_TYPE, TypeRefDescriptor::Named("String"));
    assert!(schema.directive_definition("include").is_some());
}
