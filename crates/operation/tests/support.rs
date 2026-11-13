use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    DescriptorList, ExecutableSchema, FieldDescriptor, FieldResult, OperationRootsDescriptor,
    SchemaDescriptor, TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor, Value,
};

static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "value",
        description: None,
        type_ref: "String",
        type_descriptor: STRING,
        arguments: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
    FieldDescriptor {
        name: "other",
        description: None,
        type_ref: "String",
        type_descriptor: STRING,
        arguments: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
];
static TYPES: &[TypeDescriptor] = &[TypeDescriptor {
    name: "Query",
    kind: TypeDescriptorKind::Object,
    description: None,
    fields: DescriptorList::new(&[FIELDS]),
    interfaces: DescriptorList::empty(),
    possible_types: DescriptorList::empty(),
    enum_values: DescriptorList::empty(),
    input_fields: DescriptorList::empty(),
    directives: DescriptorList::empty(),
    is_one_of: false,
    specified_by_url: None,
}];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: chigiri_runtime::GENERATED_ABI_VERSION,
    schema_digest: chigiri_runtime::SchemaDigest::from_bytes([17; 32]),
    description: None,
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

struct Dispatch;

impl ResolverDispatch<()> for Dispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        chigiri_runtime::__private::descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async { Ok(ResolvedValue::Leaf(Value::from("ok"))) })
    }
}

pub fn schema() -> ExecutableSchema<()> {
    ExecutableSchema::builder(&SCHEMA, Dispatch)
        .build()
        .unwrap()
}
