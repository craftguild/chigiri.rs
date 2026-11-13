use chigiri_runtime::{
    DescriptorList, FieldDescriptor, GENERATED_ABI_VERSION, InputValueDescriptor,
    OperationRootsDescriptor, SchemaDescriptor, SchemaDigest, TypeDescriptor, TypeDescriptorKind,
    TypeRefDescriptor,
};

static INT: TypeRefDescriptor = TypeRefDescriptor::Named("Int");
static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static PROBE: TypeRefDescriptor = TypeRefDescriptor::Named("Probe");
static EVENT: TypeRefDescriptor = TypeRefDescriptor::Named("Event");
static LIMIT: InputValueDescriptor = InputValueDescriptor {
    name: "limit",
    description: None,
    type_ref: "Int!",
    type_descriptor: TypeRefDescriptor::NonNull(&INT),
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static LIMITS: &[InputValueDescriptor] = &[
    LIMIT,
    InputValueDescriptor {
        name: "probe",
        description: None,
        type_ref: "Probe",
        type_descriptor: TypeRefDescriptor::Named("Probe"),
        default_value: None,
        default_descriptor: None,
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
];
static VALUE: InputValueDescriptor = InputValueDescriptor {
    name: "value",
    description: None,
    type_ref: "Probe!",
    type_descriptor: TypeRefDescriptor::NonNull(&PROBE),
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static VALUES: &[InputValueDescriptor] = &[VALUE];
static QUERY_FIELDS: &[FieldDescriptor] = &[
    field("ping", "String!", TypeRefDescriptor::NonNull(&STRING)),
    FieldDescriptor {
        name: "echo",
        description: None,
        type_ref: "Probe!",
        type_descriptor: TypeRefDescriptor::NonNull(&PROBE),
        arguments: DescriptorList::new(&[VALUES]),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
];
static SUBSCRIPTION_FIELDS: &[FieldDescriptor] = &[FieldDescriptor {
    name: "events",
    description: None,
    type_ref: "Event!",
    type_descriptor: TypeRefDescriptor::NonNull(&EVENT),
    arguments: DescriptorList::new(&[LIMITS]),
    directives: DescriptorList::empty(),
    deprecation_reason: None,
}];
static EVENT_FIELDS: &[FieldDescriptor] =
    &[field("value", "Int!", TypeRefDescriptor::NonNull(&INT))];
static TYPES: &[TypeDescriptor] = &[
    object("Query", DescriptorList::new(&[QUERY_FIELDS])),
    object("Mutation", DescriptorList::new(&[QUERY_FIELDS])),
    object("Subscription", DescriptorList::new(&[SUBSCRIPTION_FIELDS])),
    object("Event", DescriptorList::new(&[EVENT_FIELDS])),
    scalar("Probe"),
];
static TYPE_CHUNKS: &[&[TypeDescriptor]] = &[TYPES];

pub(super) static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: GENERATED_ABI_VERSION,
    schema_digest: SchemaDigest::from_bytes([88; 32]),
    description: None,
    schema_directives: DescriptorList::empty(),
    roots: OperationRootsDescriptor {
        query: "Query",
        mutation: Some("Mutation"),
        subscription: Some("Subscription"),
    },
    types: DescriptorList::new(TYPE_CHUNKS),
    built_in_scalar_overlays: DescriptorList::empty(),
    directives: DescriptorList::empty(),
};

const fn field(
    name: &'static str,
    type_ref: &'static str,
    type_descriptor: TypeRefDescriptor,
) -> FieldDescriptor {
    FieldDescriptor {
        name,
        description: None,
        type_ref,
        type_descriptor,
        arguments: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    }
}

const fn object(name: &'static str, fields: DescriptorList<FieldDescriptor>) -> TypeDescriptor {
    TypeDescriptor {
        name,
        kind: TypeDescriptorKind::Object,
        description: None,
        fields,
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::empty(),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    }
}

const fn scalar(name: &'static str) -> TypeDescriptor {
    TypeDescriptor {
        name,
        kind: TypeDescriptorKind::Scalar,
        description: None,
        fields: DescriptorList::empty(),
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::empty(),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    }
}
