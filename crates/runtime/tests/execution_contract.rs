use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    DescriptorList, ExecutableSchema, FieldDescriptor, FieldError, FieldResult,
    GENERATED_ABI_VERSION, GraphQlRequest, InputValueDescriptor, IntrospectionPolicy,
    OperationRootsDescriptor, SchemaDescriptor, TypeDescriptor, TypeDescriptorKind,
    TypeRefDescriptor, Value,
};

#[path = "execution/cache.rs"]
mod cache;
#[path = "execution/cancellation.rs"]
mod cancellation;
#[path = "execution/collection.rs"]
mod collection;
#[path = "execution/completion.rs"]
mod completion;
#[path = "execution/concurrency.rs"]
mod concurrency;
#[path = "execution/descriptions.rs"]
mod descriptions;
#[path = "execution/directives.rs"]
mod directives;
#[path = "execution/dispatch_integrity.rs"]
mod dispatch_integrity;
#[path = "execution/error_id_failure.rs"]
mod error_id_failure;
#[path = "execution/http_policy.rs"]
mod http_policy;
#[path = "execution/introspection.rs"]
mod introspection;
#[path = "execution/limits.rs"]
mod limits;
#[path = "execution/middleware.rs"]
mod middleware;
#[path = "execution/mutation_nested.rs"]
mod mutation_nested;
#[path = "execution/observability.rs"]
mod observability;
#[path = "execution/output_diagnostics.rs"]
mod output_diagnostics;
#[path = "execution/poll_set.rs"]
mod poll_set;
#[path = "execution/request_telemetry.rs"]
mod request_telemetry;
#[path = "execution/support.rs"]
mod support;

use support::block_on;

static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static ITEM: TypeRefDescriptor = TypeRefDescriptor::Named("Item");
static NODE: TypeRefDescriptor = TypeRefDescriptor::Named("Node");
static RESULT: TypeRefDescriptor = TypeRefDescriptor::Named("Result");
static STRING_LIST: TypeRefDescriptor = TypeRefDescriptor::List(&STRING);
static REQUIRED_STRING: TypeRefDescriptor = TypeRefDescriptor::NonNull(&STRING);
static REQUIRED_STRING_LIST: TypeRefDescriptor = TypeRefDescriptor::List(&REQUIRED_STRING);
static VALUE_ARGUMENT: InputValueDescriptor = InputValueDescriptor {
    name: "value",
    description: None,
    type_ref: "String!",
    type_descriptor: TypeRefDescriptor::NonNull(&STRING),
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
};
static VALUE_ARGUMENTS: &[InputValueDescriptor] = &[VALUE_ARGUMENT];
static QUERY_FIELDS: &[FieldDescriptor] = &[
    field("hello", "String!", TypeRefDescriptor::NonNull(&STRING)),
    FieldDescriptor {
        name: "echo",
        description: None,
        type_ref: "String!",
        type_descriptor: TypeRefDescriptor::NonNull(&STRING),
        arguments: DescriptorList::new(&[VALUE_ARGUMENTS]),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
    field("item", "Item!", TypeRefDescriptor::NonNull(&ITEM)),
    field(
        "nullableError",
        "String",
        TypeRefDescriptor::Named("String"),
    ),
    field(
        "requiredError",
        "String!",
        TypeRefDescriptor::NonNull(&STRING),
    ),
    field("nullableItems", "[String]", STRING_LIST),
    field("requiredItems", "[String!]", REQUIRED_STRING_LIST),
    field(
        "requiredList",
        "[String!]!",
        TypeRefDescriptor::NonNull(&REQUIRED_STRING_LIST),
    ),
    field("nullableItem", "Item", ITEM),
    field("requiredItem", "Item!", TypeRefDescriptor::NonNull(&ITEM)),
    field("node", "Node!", TypeRefDescriptor::NonNull(&NODE)),
    field("result", "Result!", TypeRefDescriptor::NonNull(&RESULT)),
    field("badScalar", "String", STRING),
];
static ITEM_FIELDS: &[FieldDescriptor] = &[
    field("name", "String!", TypeRefDescriptor::NonNull(&STRING)),
    field("slow", "String!", TypeRefDescriptor::NonNull(&STRING)),
    field("fast", "String!", TypeRefDescriptor::NonNull(&STRING)),
];
static MUTATION_FIELDS: &[FieldDescriptor] = &[
    FieldDescriptor {
        name: "set",
        description: None,
        type_ref: "String!",
        type_descriptor: TypeRefDescriptor::NonNull(&STRING),
        arguments: DescriptorList::new(&[VALUE_ARGUMENTS]),
        directives: DescriptorList::empty(),
        deprecation_reason: None,
    },
    field("update", "Item!", TypeRefDescriptor::NonNull(&ITEM)),
];
static TYPES: &[TypeDescriptor] = &[
    object_type("Query", DescriptorList::new(&[QUERY_FIELDS])),
    TypeDescriptor {
        name: "Item",
        kind: TypeDescriptorKind::Object,
        description: None,
        fields: DescriptorList::new(&[ITEM_FIELDS]),
        interfaces: DescriptorList::new(&[&["Node"]]),
        possible_types: DescriptorList::empty(),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    },
    TypeDescriptor {
        name: "Node",
        kind: TypeDescriptorKind::Interface,
        description: None,
        fields: DescriptorList::new(&[ITEM_FIELDS]),
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::new(&[&["Item"]]),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    },
    TypeDescriptor {
        name: "Result",
        kind: TypeDescriptorKind::Union,
        description: None,
        fields: DescriptorList::empty(),
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::new(&[&["Item"]]),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    },
    object_type("Mutation", DescriptorList::new(&[MUTATION_FIELDS])),
];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: GENERATED_ABI_VERSION,
    schema_digest: chigiri_runtime::SchemaDigest::from_bytes([2; 32]),
    description: None,
    schema_directives: DescriptorList::empty(),
    roots: OperationRootsDescriptor {
        query: "Query",
        mutation: Some("Mutation"),
        subscription: None,
    },
    types: DescriptorList::new(&[TYPES]),
    built_in_scalar_overlays: DescriptorList::empty(),
    directives: DescriptorList::empty(),
};

fn dispatch_fingerprint() -> chigiri_runtime::__private::ResolverSchemaFingerprint {
    chigiri_runtime::__private::descriptor_fingerprint(&SCHEMA)
}

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

const fn object_type(
    name: &'static str,
    fields: DescriptorList<FieldDescriptor>,
) -> TypeDescriptor {
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

struct Item {
    name: String,
    fail_name: bool,
}

struct Dispatch;

impl ResolverDispatch<()> for Dispatch {
    fn schema_fingerprint(&self) -> chigiri_runtime::__private::ResolverSchemaFingerprint {
        dispatch_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        _: &'a (),
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async move {
            let mut arguments = arguments.into_values();
            match (parent_type, field_name) {
                ("Query", "hello") => Ok(ResolvedValue::Leaf(Value::String("world".to_owned()))),
                ("Query", "echo") => {
                    let value = arguments
                        .next()
                        .and_then(|value| value.into_value())
                        .unwrap();
                    Ok(ResolvedValue::Leaf(value))
                }
                ("Query", "item") => Ok(ResolvedValue::object(
                    "Item",
                    Item {
                        name: "nested".to_owned(),
                        fail_name: false,
                    },
                )),
                ("Query", "nullableError" | "requiredError") => {
                    Err(FieldError::public("resolver failed").code("RESOLVER_FAILED"))
                }
                ("Item", "name") => {
                    let item = parent
                        .and_then(|value| value.downcast_ref::<Item>())
                        .unwrap();
                    if item.fail_name {
                        return Err(FieldError::public("nested failed").code("NESTED_FAILED"));
                    }
                    Ok(ResolvedValue::Leaf(Value::String(item.name.clone())))
                }
                ("Item", "slow" | "fast") => Ok(ResolvedValue::Leaf(Value::from(field_name))),
                ("Query", "nullableItems" | "requiredItems" | "requiredList") => {
                    Ok(ResolvedValue::List(vec![
                        Ok(ResolvedValue::Leaf(Value::from("first"))),
                        Err(FieldError::public("item failed").code("ITEM_FAILED")),
                        Ok(ResolvedValue::Leaf(Value::from("third"))),
                    ]))
                }
                ("Query", "nullableItem" | "requiredItem") => Ok(ResolvedValue::object(
                    "Item",
                    Item {
                        name: "unused".to_owned(),
                        fail_name: true,
                    },
                )),
                ("Query", "node" | "result") => Ok(ResolvedValue::object(
                    "Item",
                    Item {
                        name: "abstract".to_owned(),
                        fail_name: false,
                    },
                )),
                ("Query", "badScalar") => Ok(ResolvedValue::Leaf(Value::Int(1))),
                ("Mutation", "set") => {
                    let value = arguments
                        .next()
                        .and_then(|value| value.into_value())
                        .unwrap();
                    Ok(ResolvedValue::Leaf(value))
                }
                _ => Err(FieldError::public("unknown dispatch")),
            }
        })
    }
}

fn schema() -> ExecutableSchema<()> {
    ExecutableSchema::builder(&SCHEMA, Dispatch)
        .introspection(IntrospectionPolicy::Disabled)
        .build()
        .unwrap()
}

#[test]
fn exposes_the_abi5_descriptor_schema_digest_as_immutable_schema_info() {
    let schema = schema();
    let info = schema.schema_info();

    assert_eq!(info.framework_version(), env!("CARGO_PKG_VERSION"));
    assert_eq!(info.generated_abi(), Some(GENERATED_ABI_VERSION));
    assert_eq!(info.schema_digest(), SCHEMA.schema_digest);
    assert!(info.catalog_revision().is_none());
}

#[test]
fn aliases_preserve_nested_object_response_order() {
    let response =
        block_on(schema().execute(GraphQlRequest::new("{ z: hello, a: item { name } }"), ()));
    let Some(Value::Object(data)) = response.data else {
        panic!("expected object data");
    };

    assert!(response.errors.is_empty());
    assert_eq!(
        data.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["z", "a"]
    );
    assert_eq!(data.get("z"), Some(&Value::String("world".to_owned())));
    let Some(Value::Object(item)) = data.get("a") else {
        panic!("expected nested objects");
    };
    assert_eq!(item.get("name"), Some(&Value::String("nested".to_owned())));
}

#[test]
fn distinguishes_request_errors_from_nullable_non_null_completion() {
    let invalid = block_on(schema().execute(GraphQlRequest::new("{ missing }"), ()));
    let nullable = block_on(schema().execute(GraphQlRequest::new("{ nullableError hello }"), ()));
    let required = block_on(schema().execute(GraphQlRequest::new("{ requiredError hello }"), ()));

    assert!(invalid.data.is_none());
    assert_eq!(
        invalid.errors[0].extensions.get("code"),
        Some(&Value::String("GRAPHQL_VALIDATION_FAILED".to_owned()))
    );
    let Some(Value::Object(nullable_data)) = nullable.data else {
        panic!("expected nullable response object");
    };
    assert_eq!(nullable_data.get("nullableError"), Some(&Value::Null));
    assert_eq!(
        nullable_data.get("hello"),
        Some(&Value::String("world".to_owned()))
    );
    assert_eq!(nullable.errors[0].path, ["nullableError".into()]);
    assert_eq!(required.data, Some(Value::Null));
    assert_eq!(required.errors[0].path, ["requiredError".into()]);
}
