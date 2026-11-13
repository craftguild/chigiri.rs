//! September 2025 § 4.2 to maintain normative introspection object signatures.

use super::model::{
    DefaultValueDescriptor, FieldDescriptor, InputValueDescriptor, LIST_OF_NON_NULL, NON_NULL,
    NON_NULL_LIST_OF_NON_NULL, NULLABLE, TypeDescriptor, TypeRefDescriptor, field, object,
};

const NO_ARGUMENTS: &[InputValueDescriptor] = &[];
const TYPE_NAME_ARGUMENT: &[InputValueDescriptor] = &[InputValueDescriptor {
    name: "name",
    description: None,
    ty: TypeRefDescriptor {
        named_type: "String",
        wrappers: NON_NULL,
    },
    default_value: None,
}];
const INCLUDE_DEPRECATED: &[InputValueDescriptor] = &[InputValueDescriptor {
    name: "includeDeprecated",
    description: None,
    ty: TypeRefDescriptor {
        named_type: "Boolean",
        wrappers: NON_NULL,
    },
    default_value: Some(DefaultValueDescriptor::Boolean(false)),
}];

const SCHEMA_FIELDS: &[FieldDescriptor] = &[
    field("description", "String", NULLABLE, NO_ARGUMENTS),
    field("types", "__Type", NON_NULL_LIST_OF_NON_NULL, NO_ARGUMENTS),
    field("queryType", "__Type", NON_NULL, NO_ARGUMENTS),
    field("mutationType", "__Type", NULLABLE, NO_ARGUMENTS),
    field("subscriptionType", "__Type", NULLABLE, NO_ARGUMENTS),
    field(
        "directives",
        "__Directive",
        NON_NULL_LIST_OF_NON_NULL,
        NO_ARGUMENTS,
    ),
];

const TYPE_FIELDS: &[FieldDescriptor] = &[
    field("kind", "__TypeKind", NON_NULL, NO_ARGUMENTS),
    field("name", "String", NULLABLE, NO_ARGUMENTS),
    field("description", "String", NULLABLE, NO_ARGUMENTS),
    field("specifiedByURL", "String", NULLABLE, NO_ARGUMENTS),
    field("fields", "__Field", LIST_OF_NON_NULL, INCLUDE_DEPRECATED),
    field("interfaces", "__Type", LIST_OF_NON_NULL, NO_ARGUMENTS),
    field("possibleTypes", "__Type", LIST_OF_NON_NULL, NO_ARGUMENTS),
    field(
        "enumValues",
        "__EnumValue",
        LIST_OF_NON_NULL,
        INCLUDE_DEPRECATED,
    ),
    field(
        "inputFields",
        "__InputValue",
        LIST_OF_NON_NULL,
        INCLUDE_DEPRECATED,
    ),
    field("ofType", "__Type", NULLABLE, NO_ARGUMENTS),
    field("isOneOf", "Boolean", NULLABLE, NO_ARGUMENTS),
];

const FIELD_FIELDS: &[FieldDescriptor] = &[
    field("name", "String", NON_NULL, NO_ARGUMENTS),
    field("description", "String", NULLABLE, NO_ARGUMENTS),
    field(
        "args",
        "__InputValue",
        NON_NULL_LIST_OF_NON_NULL,
        INCLUDE_DEPRECATED,
    ),
    field("type", "__Type", NON_NULL, NO_ARGUMENTS),
    field("isDeprecated", "Boolean", NON_NULL, NO_ARGUMENTS),
    field("deprecationReason", "String", NULLABLE, NO_ARGUMENTS),
];

const INPUT_VALUE_FIELDS: &[FieldDescriptor] = &[
    field("name", "String", NON_NULL, NO_ARGUMENTS),
    field("description", "String", NULLABLE, NO_ARGUMENTS),
    field("type", "__Type", NON_NULL, NO_ARGUMENTS),
    field("defaultValue", "String", NULLABLE, NO_ARGUMENTS),
    field("isDeprecated", "Boolean", NON_NULL, NO_ARGUMENTS),
    field("deprecationReason", "String", NULLABLE, NO_ARGUMENTS),
];

const ENUM_VALUE_FIELDS: &[FieldDescriptor] = &[
    field("name", "String", NON_NULL, NO_ARGUMENTS),
    field("description", "String", NULLABLE, NO_ARGUMENTS),
    field("isDeprecated", "Boolean", NON_NULL, NO_ARGUMENTS),
    field("deprecationReason", "String", NULLABLE, NO_ARGUMENTS),
];

const DIRECTIVE_FIELDS: &[FieldDescriptor] = &[
    field("name", "String", NON_NULL, NO_ARGUMENTS),
    field("description", "String", NULLABLE, NO_ARGUMENTS),
    field("isRepeatable", "Boolean", NON_NULL, NO_ARGUMENTS),
    field(
        "locations",
        "__DirectiveLocation",
        NON_NULL_LIST_OF_NON_NULL,
        NO_ARGUMENTS,
    ),
    field(
        "args",
        "__InputValue",
        NON_NULL_LIST_OF_NON_NULL,
        INCLUDE_DEPRECATED,
    ),
];

pub(super) const SCHEMA: TypeDescriptor = object("__Schema", SCHEMA_FIELDS);
pub(super) const TYPE: TypeDescriptor = object("__Type", TYPE_FIELDS);
pub(super) const FIELD: TypeDescriptor = object("__Field", FIELD_FIELDS);
pub(super) const INPUT_VALUE: TypeDescriptor = object("__InputValue", INPUT_VALUE_FIELDS);
pub(super) const ENUM_VALUE: TypeDescriptor = object("__EnumValue", ENUM_VALUE_FIELDS);
pub(super) const DIRECTIVE: TypeDescriptor = object("__Directive", DIRECTIVE_FIELDS);

const QUERY_META_FIELDS: &[FieldDescriptor] = &[
    field("__schema", "__Schema", NON_NULL, NO_ARGUMENTS),
    field("__type", "__Type", NULLABLE, TYPE_NAME_ARGUMENT),
];

pub(super) fn root_field(name: &str) -> Option<&'static FieldDescriptor> {
    QUERY_META_FIELDS.iter().find(|field| field.name == name)
}
