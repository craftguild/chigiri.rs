use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    ConstValueDescriptor, DescriptorList, ExecutableSchemaBuilder, FieldDescriptor, FieldResult,
    InputValueDescriptor, OperationRootsDescriptor, SchemaDescriptor, TypeDescriptor,
    TypeDescriptorKind, TypeRefDescriptor, Value,
};

static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static INT: TypeRefDescriptor = TypeRefDescriptor::Named("Int");
static GROUP: TypeRefDescriptor = TypeRefDescriptor::Named("Group");
static GROUP_LIST: TypeRefDescriptor = TypeRefDescriptor::List(&GROUP);
static ITEM: TypeRefDescriptor = TypeRefDescriptor::Named("Item");
static ITEM_LIST: TypeRefDescriptor = TypeRefDescriptor::List(&ITEM);
static FIRST_ARGUMENT: InputValueDescriptor = InputValueDescriptor {
    name: "first",
    description: None,
    type_ref: "Int",
    type_descriptor: INT,
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static FIRST_ARGUMENTS: &[InputValueDescriptor] = &[FIRST_ARGUMENT];
static DEFAULT_FIRST_ARGUMENT: InputValueDescriptor = InputValueDescriptor {
    name: "first",
    description: None,
    type_ref: "Int",
    type_descriptor: INT,
    default_value: Some("2"),
    default_descriptor: Some(ConstValueDescriptor::Int("2")),
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static DEFAULT_FIRST_ARGUMENTS: &[InputValueDescriptor] = &[DEFAULT_FIRST_ARGUMENT];
static QUERY_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "hello",
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
    FieldDescriptor {
        name: "items",
        description: None,
        type_ref: "String",
        type_descriptor: STRING,
        arguments: DescriptorList::new(&[FIRST_ARGUMENTS]),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
    FieldDescriptor {
        name: "groups",
        description: None,
        type_ref: "[Group]",
        type_descriptor: GROUP_LIST,
        arguments: DescriptorList::new(&[DEFAULT_FIRST_ARGUMENTS]),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
];
static GROUP_FIELDS: &[FieldDescriptor] = &[FieldDescriptor {
    name: "items",
    description: None,
    type_ref: "[Item]",
    type_descriptor: ITEM_LIST,
    arguments: DescriptorList::new(&[FIRST_ARGUMENTS]),
    directives: DescriptorList::empty(),
    deprecation_reason: None,
}];
static ITEM_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "children",
        description: None,
        type_ref: "[Item]",
        type_descriptor: ITEM_LIST,
        arguments: DescriptorList::new(&[FIRST_ARGUMENTS]),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
    FieldDescriptor {
        name: "expensive",
        description: None,
        type_ref: "String",
        type_descriptor: STRING,
        arguments: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
];
static TYPES: &[TypeDescriptor] = &[
    object("Query", DescriptorList::new(&[QUERY_FIELDS])),
    object("Group", DescriptorList::new(&[GROUP_FIELDS])),
    object("Item", DescriptorList::new(&[ITEM_FIELDS])),
];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: chigiri_runtime::GENERATED_ABI_VERSION,
    schema_digest: chigiri_runtime::SchemaDigest::from_bytes([31; 32]),
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

#[derive(Clone)]
pub struct TestContext;

struct Dispatch {
    calls: Arc<AtomicUsize>,
}

impl ResolverDispatch<TestContext> for Dispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        chigiri_runtime::__private::descriptor_fingerprint(&SCHEMA)
    }
    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}
    fn resolve<'a>(
        &'a self,
        _: &'a TestContext,
        _: &'a str,
        _: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(ResolvedValue::Leaf(Value::from("ok"))) })
    }
}

pub fn builder(calls: Arc<AtomicUsize>) -> ExecutableSchemaBuilder<TestContext> {
    chigiri_runtime::ExecutableSchema::builder(&SCHEMA, Dispatch { calls })
}
