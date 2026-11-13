use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

use chigiri_core::executable::CustomScalarRegistry;
use chigiri_core::executor::{
    ArgumentValues, BoxFuture, FieldContext, ResolvedObject, ResolvedValue,
};
use chigiri_core::{FieldResult, GraphQlRequest};

use crate::private::{
    ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent, ResolverDispatch,
    ResolverSchemaFingerprint, SubscriptionResolverDispatch, descriptor_fingerprint,
    invariant_error,
};
use crate::{
    DescriptorList, FieldDescriptor, GENERATED_ABI_VERSION, OperationRootsDescriptor,
    SchemaDescriptor, SchemaDigest, TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor,
};

use super::ExecutableSchema;

const fn field(name: &'static str) -> FieldDescriptor {
    FieldDescriptor {
        name,
        description: None,
        type_ref: "String",
        type_descriptor: TypeRefDescriptor::Named("String"),
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

static TYPES: &[TypeDescriptor] = &[
    object("Query", DescriptorList::new(&[&[field("hello")]])),
    object("Subscription", DescriptorList::new(&[&[field("event")]])),
];
static TYPE_CHUNKS: &[&[TypeDescriptor]] = &[TYPES];

const fn descriptor(subscription: Option<&'static str>) -> SchemaDescriptor {
    SchemaDescriptor {
        abi_version: GENERATED_ABI_VERSION,
        schema_digest: SchemaDigest::from_bytes([31; 32]),
        description: None,
        schema_directives: DescriptorList::empty(),
        roots: OperationRootsDescriptor {
            query: "Query",
            mutation: None,
            subscription,
        },
        types: DescriptorList::new(TYPE_CHUNKS),
        built_in_scalar_overlays: DescriptorList::empty(),
        directives: DescriptorList::empty(),
    }
}

static SUBSCRIPTION: SchemaDescriptor = descriptor(Some("Subscription"));
static FINITE: SchemaDescriptor = descriptor(None);

struct OrdinaryOnly(&'static SchemaDescriptor);

impl ResolverDispatch<()> for OrdinaryOnly {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        descriptor_fingerprint(self.0)
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
        Box::pin(async { Ok(ResolvedValue::Leaf("hello".into())) })
    }
}

struct Complete {
    ordinary: OrdinaryOnly,
    root: Option<&'static str>,
    setup_calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}

impl Drop for Complete {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl ResolverDispatch<()> for Complete {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        self.ordinary.schema_fingerprint()
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        context: &'a (),
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.ordinary
            .resolve(context, parent_type, field_name, parent, arguments)
    }
}

impl SubscriptionResolverDispatch<()> for Complete {
    fn subscription_root(&self) -> Option<&'static str> {
        self.root
    }

    fn subscribe<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        self.setup_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(invariant_error(
                "setup must not run while building a schema",
            ))
        })
    }

    fn resolve_event<'a>(
        &'a self,
        _: FieldContext<'a, ()>,
        _: &'a ErasedSubscriptionArguments,
        _: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        Box::pin(async {
            Err(invariant_error(
                "event must not run while building a schema",
            ))
        })
    }
}

fn complete(descriptor: &'static SchemaDescriptor, root: Option<&'static str>) -> Complete {
    Complete {
        ordinary: OrdinaryOnly(descriptor),
        root,
        setup_calls: Arc::new(AtomicUsize::new(0)),
        drops: Arc::new(AtomicUsize::new(0)),
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("fixture must complete without an executor"),
    }
}

#[test]
fn generated_capabilities_share_one_allocation_and_survive_reconfiguration() {
    let dispatch = complete(&SUBSCRIPTION, Some("Subscription"));
    let calls = Arc::clone(&dispatch.setup_calls);
    let drops = Arc::clone(&dispatch.drops);
    let builder = ExecutableSchema::builder_with_subscriptions(&SUBSCRIPTION, dispatch);
    assert_eq!(
        Arc::as_ptr(&builder.resolvers) as *const (),
        Arc::as_ptr(builder.subscription.as_ref().unwrap()) as *const (),
    );
    let schema = builder.build().unwrap();
    let cloned = schema.clone();
    let rebuilt = schema.reconfigure().build();
    assert!(Arc::ptr_eq(
        schema.inner.subscription.as_ref().unwrap(),
        rebuilt.inner.subscription.as_ref().unwrap(),
    ));
    let response = block_on(rebuilt.execute(GraphQlRequest::new("{ hello }"), ()));
    assert!(response.errors.is_empty(), "{response:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(schema);
    drop(cloned);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(rebuilt);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn generated_root_mismatch_fails_during_build() {
    for (descriptor, root) in [
        (&SUBSCRIPTION, None),
        (&SUBSCRIPTION, Some("Other")),
        (&FINITE, Some("Subscription")),
    ] {
        let error =
            ExecutableSchema::builder_with_subscriptions(descriptor, complete(descriptor, root))
                .build()
                .err()
                .expect("root disagreement must fail");
        assert_eq!(
            error.to_string(),
            "generated descriptor and subscription dispatch root do not match"
        );
    }
}

#[test]
fn finite_manual_builder_requires_only_ordinary_dispatch() {
    let schema = ExecutableSchema::builder(&FINITE, OrdinaryOnly(&FINITE))
        .build()
        .unwrap();
    assert!(schema.inner.subscription.is_none());
    let response = block_on(schema.execute(GraphQlRequest::new("{ hello }"), ()));
    assert!(response.errors.is_empty(), "{response:?}");
    assert!(
        ExecutableSchema::builder(&SUBSCRIPTION, OrdinaryOnly(&SUBSCRIPTION))
            .build()
            .is_err()
    );
}

#[test]
fn generated_assembly_without_a_subscription_root_is_valid() {
    let schema = ExecutableSchema::builder_with_subscriptions(&FINITE, complete(&FINITE, None))
        .build()
        .unwrap();
    assert_eq!(
        schema
            .inner
            .subscription
            .as_ref()
            .unwrap()
            .subscription_root(),
        None
    );
}
