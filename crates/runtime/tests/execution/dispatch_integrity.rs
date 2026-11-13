//! Verifies that identical field coordinates still reject an adapter signature from another descriptor.

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    DescriptorList, ExecutableSchema, FieldDescriptor, FieldResult, GENERATED_ABI_VERSION,
    InputValueDescriptor, OperationRootsDescriptor, SchemaDescriptor, TypeDescriptor,
    TypeDescriptorKind, TypeRefDescriptor,
};

static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static REQUIRED_STRING: TypeRefDescriptor = TypeRefDescriptor::NonNull(&STRING);
static ARGUMENT: InputValueDescriptor = InputValueDescriptor {
    name: "input",
    description: None,
    type_ref: "String!",
    type_descriptor: REQUIRED_STRING,
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static ARGUMENTS: &[InputValueDescriptor] = &[ARGUMENT];
static BASE_FIELDS: &[FieldDescriptor] =
    &[field("value", "String", STRING, DescriptorList::empty())];
static RETURN_CHANGED_FIELDS: &[FieldDescriptor] = &[field(
    "value",
    "String!",
    REQUIRED_STRING,
    DescriptorList::empty(),
)];
static ARG_CHANGED_FIELDS: &[FieldDescriptor] = &[field(
    "value",
    "String",
    STRING,
    DescriptorList::new(&[ARGUMENTS]),
)];
static BASE_TYPE: TypeDescriptor = object(DescriptorList::new(&[BASE_FIELDS]));
static RETURN_CHANGED_TYPE: TypeDescriptor = object(DescriptorList::new(&[RETURN_CHANGED_FIELDS]));
static ARG_CHANGED_TYPE: TypeDescriptor = object(DescriptorList::new(&[ARG_CHANGED_FIELDS]));
static BASE_TYPES: &[TypeDescriptor] = &[BASE_TYPE];
static RETURN_CHANGED_TYPES: &[TypeDescriptor] = &[RETURN_CHANGED_TYPE];
static ARG_CHANGED_TYPES: &[TypeDescriptor] = &[ARG_CHANGED_TYPE];
static BASE_SCHEMA: SchemaDescriptor = schema(DescriptorList::new(&[BASE_TYPES]));
static RETURN_CHANGED_SCHEMA: SchemaDescriptor =
    schema(DescriptorList::new(&[RETURN_CHANGED_TYPES]));
static ARG_CHANGED_SCHEMA: SchemaDescriptor = schema(DescriptorList::new(&[ARG_CHANGED_TYPES]));

const fn field(
    name: &'static str,
    type_ref: &'static str,
    type_descriptor: TypeRefDescriptor,
    arguments: DescriptorList<InputValueDescriptor>,
) -> FieldDescriptor {
    FieldDescriptor {
        name,
        description: None,
        type_ref,
        type_descriptor,
        arguments,
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    }
}

const fn object(fields: DescriptorList<FieldDescriptor>) -> TypeDescriptor {
    TypeDescriptor {
        name: "Query",
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

const fn schema(types: DescriptorList<TypeDescriptor>) -> SchemaDescriptor {
    SchemaDescriptor {
        abi_version: GENERATED_ABI_VERSION,
        schema_digest: chigiri_runtime::SchemaDigest::from_bytes([4; 32]),
        description: None,
        schema_directives: DescriptorList::empty(),
        roots: OperationRootsDescriptor {
            query: "Query",
            mutation: None,
            subscription: None,
        },
        types,
        built_in_scalar_overlays: DescriptorList::empty(),
        directives: DescriptorList::empty(),
    }
}

struct BaseDispatch;

impl ResolverDispatch<()> for BaseDispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        chigiri_runtime::__private::descriptor_fingerprint(&BASE_SCHEMA)
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
        Box::pin(async { Ok(ResolvedValue::Null) })
    }
}

#[test]
fn startup_rejects_return_type_changes_for_the_same_coordinate() {
    assert_mismatch(&RETURN_CHANGED_SCHEMA);
}

#[test]
fn startup_rejects_argument_signature_changes_for_the_same_coordinate() {
    assert_mismatch(&ARG_CHANGED_SCHEMA);
}

#[test]
fn rejects_resolver_abi_fingerprint_mismatches_at_startup() {
    let schema = chigiri_runtime::__private::descriptor_fingerprint(&BASE_SCHEMA);
    let changed = chigiri_runtime::__private::ResolverSchemaFingerprint::new(
        schema.component_count() + 1,
        schema.hash(),
    );
    let result = ExecutableSchema::builder(&BASE_SCHEMA, BaseDispatch)
        .resolver_abi_fingerprint(changed)
        .build();
    let Err(error) = result else {
        panic!("expected an ABI fingerprint mismatch configuration");
    };
    assert!(error.to_string().contains("resolver ABI fingerprint"));
}

fn assert_mismatch(descriptor: &'static SchemaDescriptor) {
    let result = ExecutableSchema::builder(descriptor, BaseDispatch).build();
    let Err(error) = result else {
        panic!("unexpected type-adapter signature");
    };
    assert!(error.to_string().contains("resolver dispatch table"));
}
