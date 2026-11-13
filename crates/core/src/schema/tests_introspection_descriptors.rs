use super::introspection::model::{
    DefaultValueDescriptor, DescriptorKind, FieldDescriptor, TypeRefDescriptor, TypeWrapper,
};
use super::introspection::{type_by_name, types};

fn render_type(reference: TypeRefDescriptor) -> String {
    let mut rendered = reference.named_type.to_owned();
    for wrapper in reference.wrappers.iter().rev() {
        match wrapper {
            TypeWrapper::List => rendered = format!("[{rendered}]"),
            TypeWrapper::NonNull => rendered.push('!'),
        }
    }
    rendered
}

fn render_field(field: &FieldDescriptor) -> String {
    let arguments = if field.arguments.is_empty() {
        String::new()
    } else {
        let rendered = field
            .arguments
            .iter()
            .map(|argument| {
                let default = match argument.default_value {
                    Some(DefaultValueDescriptor::Boolean(value)) => format!(" = {value}"),
                    None => String::new(),
                };
                format!("{}: {}{default}", argument.name, render_type(argument.ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("({rendered})")
    };
    format!("{}{arguments}: {}", field.name, render_type(field.ty))
}

fn signatures(name: &str) -> Vec<String> {
    type_by_name(name)
        .unwrap()
        .fields
        .iter()
        .map(render_field)
        .collect()
}

#[test]
fn exposes_eight_types_in_stable_order_with_lookup() {
    assert_eq!(
        types()
            .iter()
            .map(|descriptor| descriptor.name)
            .collect::<Vec<_>>(),
        [
            "__Schema",
            "__Type",
            "__Field",
            "__InputValue",
            "__EnumValue",
            "__Directive",
            "__TypeKind",
            "__DirectiveLocation",
        ]
    );
    assert!(std::ptr::eq(type_by_name("__Type").unwrap(), &types()[1]));
    assert!(type_by_name("__Missing").is_none());
}

#[test]
fn schema_and_type_introspection_field_signatures_are_exact() {
    assert_eq!(
        signatures("__Schema"),
        [
            "description: String",
            "types: [__Type!]!",
            "queryType: __Type!",
            "mutationType: __Type",
            "subscriptionType: __Type",
            "directives: [__Directive!]!",
        ]
    );
    assert_eq!(
        signatures("__Type"),
        [
            "kind: __TypeKind!",
            "name: String",
            "description: String",
            "specifiedByURL: String",
            "fields(includeDeprecated: Boolean! = false): [__Field!]",
            "interfaces: [__Type!]",
            "possibleTypes: [__Type!]",
            "enumValues(includeDeprecated: Boolean! = false): [__EnumValue!]",
            "inputFields(includeDeprecated: Boolean! = false): [__InputValue!]",
            "ofType: __Type",
            "isOneOf: Boolean",
        ]
    );
}

#[test]
fn member_descriptors_preserve_exact_field_signatures() {
    assert_eq!(
        signatures("__Field"),
        [
            "name: String!",
            "description: String",
            "args(includeDeprecated: Boolean! = false): [__InputValue!]!",
            "type: __Type!",
            "isDeprecated: Boolean!",
            "deprecationReason: String",
        ]
    );
    assert_eq!(
        signatures("__InputValue"),
        [
            "name: String!",
            "description: String",
            "type: __Type!",
            "defaultValue: String",
            "isDeprecated: Boolean!",
            "deprecationReason: String",
        ]
    );
    assert_eq!(
        signatures("__EnumValue"),
        [
            "name: String!",
            "description: String",
            "isDeprecated: Boolean!",
            "deprecationReason: String",
        ]
    );
    assert_eq!(
        signatures("__Directive"),
        [
            "name: String!",
            "description: String",
            "isRepeatable: Boolean!",
            "locations: [__DirectiveLocation!]!",
            "args(includeDeprecated: Boolean! = false): [__InputValue!]!",
        ]
    );
}

#[test]
fn preserves_september_2025_enum_value_order() {
    let values = |name| {
        type_by_name(name)
            .unwrap()
            .enum_values
            .iter()
            .map(|value| value.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        values("__TypeKind"),
        [
            "SCALAR",
            "OBJECT",
            "INTERFACE",
            "UNION",
            "ENUM",
            "INPUT_OBJECT",
            "LIST",
            "NON_NULL",
        ]
    );
    assert_eq!(
        values("__DirectiveLocation"),
        [
            "QUERY",
            "MUTATION",
            "SUBSCRIPTION",
            "FIELD",
            "FRAGMENT_DEFINITION",
            "FRAGMENT_SPREAD",
            "INLINE_FRAGMENT",
            "VARIABLE_DEFINITION",
            "SCHEMA",
            "SCALAR",
            "OBJECT",
            "FIELD_DEFINITION",
            "ARGUMENT_DEFINITION",
            "INTERFACE",
            "UNION",
            "ENUM",
            "ENUM_VALUE",
            "INPUT_OBJECT",
            "INPUT_FIELD_DEFINITION",
        ]
    );
}

#[test]
fn include_deprecated_defaults_to_false_in_all_five_locations() {
    let arguments = types()
        .iter()
        .flat_map(|descriptor| descriptor.fields)
        .flat_map(|field| field.arguments)
        .filter(|argument| argument.name == "includeDeprecated")
        .collect::<Vec<_>>();

    assert_eq!(arguments.len(), 5);
    assert!(arguments.iter().all(|argument| {
        argument.ty.named_type == "Boolean"
            && argument.ty.wrappers == [TypeWrapper::NonNull]
            && argument.default_value == Some(DefaultValueDescriptor::Boolean(false))
    }));
}

#[test]
fn descriptors_do_not_retain_source_text() {
    assert!(types().iter().all(|descriptor| {
        descriptor.description.is_none()
            && descriptor
                .fields
                .iter()
                .all(|field| field.description.is_none())
            && descriptor
                .enum_values
                .iter()
                .all(|value| value.description.is_none())
    }));
    assert_eq!(
        type_by_name("__Schema").unwrap().kind,
        DescriptorKind::Object
    );
    assert_eq!(
        type_by_name("__TypeKind").unwrap().kind,
        DescriptorKind::Enum
    );
}
