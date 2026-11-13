//! Small handwritten schema for retained-metadata and preparation contract tests.

use chigiri_http::{ApplicationMetadata, RequestHeader};
use chigiri_runtime::__private::{
    ArgumentValues, BoxFuture, ErasedSubscription, ErasedSubscriptionArguments,
    ErasedSubscriptionEvent, PreparedSubscription, ResolvedObject, ResolvedValue, ResolverDispatch,
    ResolverSchemaFingerprint, SubscriptionResolverDispatch, descriptor_fingerprint,
    invariant_error,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{
    DescriptorList, ExecutableSchema, FieldContext, FieldDescriptor, FieldResult,
    GENERATED_ABI_VERSION, GraphQlRequest, InputValueDescriptor, OperationRootsDescriptor,
    SchemaDescriptor, SchemaDigest, TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor,
};

use super::{
    MetadataRetentionLimit, RequestMethod, ResponseMediaType, SubscriberRequest,
    SubscriptionTransport, TransportRequest,
};

#[path = "test_support/host.rs"]
pub(crate) mod host;

static ARGUMENTS: &[InputValueDescriptor] = &[InputValueDescriptor {
    name: "message",
    description: None,
    type_ref: "String",
    type_descriptor: TypeRefDescriptor::Named("String"),
    default_value: None,
    default_descriptor: None,
    directives: DescriptorList::empty(),
    deprecation_reason: None,
}];
static QUERY: &[FieldDescriptor] = &[field("ready", DescriptorList::empty())];
static SUBSCRIPTION: &[FieldDescriptor] = &[field("event", DescriptorList::new(&[ARGUMENTS]))];
static TYPES: &[TypeDescriptor] = &[
    object("Query", DescriptorList::new(&[QUERY])),
    object("Subscription", DescriptorList::new(&[SUBSCRIPTION])),
];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: GENERATED_ABI_VERSION,
    schema_digest: SchemaDigest::from_bytes([54; 32]),
    description: None,
    schema_directives: DescriptorList::empty(),
    roots: OperationRootsDescriptor {
        query: "Query",
        mutation: None,
        subscription: Some("Subscription"),
    },
    types: DescriptorList::new(&[TYPES]),
    built_in_scalar_overlays: DescriptorList::empty(),
    directives: DescriptorList::empty(),
};

const fn field(
    name: &'static str,
    arguments: DescriptorList<InputValueDescriptor>,
) -> FieldDescriptor {
    FieldDescriptor {
        name,
        description: None,
        type_ref: "String",
        type_descriptor: TypeRefDescriptor::Named("String"),
        arguments,
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

struct Dispatch(Option<Arc<host::Probe>>);

use std::sync::Arc;

impl ResolverDispatch<()> for Dispatch {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        descriptor_fingerprint(&SCHEMA)
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
        Box::pin(async { Err(invariant_error("metadata fixture must not execute fields")) })
    }
}

impl SubscriptionResolverDispatch<()> for Dispatch {
    fn prepare_observation(
        &self,
        _: &(),
        _: &str,
        _: &str,
        _: chigiri_runtime::__private::SubscriptionObservationReceipt,
        available: usize,
    ) -> FieldResult<Option<Box<dyn chigiri_runtime::__private::HostSubscriptionObservation>>> {
        self.0
            .as_ref()
            .map_or(Ok(None), |probe| host::prepare(probe, available).map(Some))
    }
    fn subscription_root(&self) -> Option<&'static str> {
        Some("Subscription")
    }
    fn subscribe<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        Box::pin(async {
            Err(invariant_error(
                "metadata fixture must not initialize sources",
            ))
        })
    }
    fn resolve_event<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        _: &'a ErasedSubscriptionArguments,
        _: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async { Err(invariant_error("metadata fixture must not execute events")) })
    }
}

pub(crate) fn schema() -> ExecutableSchema<()> {
    builder().build().unwrap()
}

pub(crate) fn builder() -> chigiri_runtime::ExecutableSchemaBuilder<()> {
    ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(None))
}

#[cfg(feature = "subscription-worker")]
pub(crate) fn with_host(probe: Arc<host::Probe>) -> ExecutableSchema<()> {
    ExecutableSchema::builder_with_subscriptions(&SCHEMA, Dispatch(Some(probe)))
        .build()
        .unwrap()
}

pub(crate) fn prepared(request: GraphQlRequest) -> PreparedSubscription<()> {
    schema().prepare_subscription(request).unwrap()
}

pub(crate) fn graph_request() -> GraphQlRequest {
    GraphQlRequest::new("subscription Named($message: String) { event(message: $message) }")
        .with_operation_name("Named")
}

pub(crate) fn subscriber(profile: SubscriptionTransport) -> SubscriberRequest {
    let metadata = ApplicationMetadata::default();
    let headers = [RequestHeader::new("authorization", b"original-secret")];
    let transport = TransportRequest::new(
        "POST",
        "/graphql",
        Some("private=query"),
        &headers,
        None,
        &metadata,
    );
    SubscriberRequest::retain(
        transport,
        &prepared(graph_request()),
        RequestMethod::Post,
        ResponseMediaType::GraphQlResponseJson,
        profile,
        None,
        MetadataRetentionLimit::default(),
    )
    .unwrap()
}
