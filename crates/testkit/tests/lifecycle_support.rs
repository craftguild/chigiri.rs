use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use chigiri::__private::{
    ArgumentValues, BoxFuture, ResolvedObject, ResolvedValue, ResolverDispatch,
};
use chigiri::core::executable::CustomScalarRegistry;
use chigiri::spi::{ExecutableSchema, ExecutableSchemaBuilder};
use chigiri::{
    DescriptorList, Endpoint, FieldDescriptor, FieldError, FieldResult, GraphQlError,
    GraphQlResponse, MissingContextEndpointBuilder, OperationContext, OperationNext,
    OperationRootsDescriptor, Plugin, RequestContext, RequestNext, SchemaDescriptor,
    TypeDescriptor, TypeDescriptorKind, TypeRefDescriptor, Value,
};
use chigiri_http::{
    AdmissionDecision, AdmissionError, DecodedRequestHook, DecodedRequestView,
    OperationSourceResolver, Rejection, RequestEnvelope, ResolvedOperationSource,
    SourceResolutionError, TransportAdmission, TransportRequest,
};

static STRING: TypeRefDescriptor = TypeRefDescriptor::Named("String");
static STRING_LIST: TypeRefDescriptor = TypeRefDescriptor::List(&STRING);
static ITEM: TypeRefDescriptor = TypeRefDescriptor::Named("Item");
static QUERY_FIELDS: &[FieldDescriptor] = &[
    field("hello"),
    field("fail"),
    typed_field("items", "[String]", STRING_LIST),
    typed_field("item", "Item", ITEM),
];
static MUTATION_FIELDS: &[FieldDescriptor] = &[field("touch")];
static ITEM_FIELDS: &[FieldDescriptor] = &[field("name")];
static QUERY_FIELD_GROUPS: &[&[FieldDescriptor]] = &[QUERY_FIELDS];
static MUTATION_FIELD_GROUPS: &[&[FieldDescriptor]] = &[MUTATION_FIELDS];
static ITEM_FIELD_GROUPS: &[&[FieldDescriptor]] = &[ITEM_FIELDS];
static TYPES: &[TypeDescriptor] = &[
    TypeDescriptor {
        name: "Query",
        kind: TypeDescriptorKind::Object,
        description: None,
        fields: DescriptorList::new(QUERY_FIELD_GROUPS),
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::empty(),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    },
    TypeDescriptor {
        name: "Mutation",
        kind: TypeDescriptorKind::Object,
        description: None,
        fields: DescriptorList::new(MUTATION_FIELD_GROUPS),
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::empty(),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    },
    TypeDescriptor {
        name: "Item",
        kind: TypeDescriptorKind::Object,
        description: None,
        fields: DescriptorList::new(ITEM_FIELD_GROUPS),
        interfaces: DescriptorList::empty(),
        possible_types: DescriptorList::empty(),
        enum_values: DescriptorList::empty(),
        input_fields: DescriptorList::empty(),
        directives: DescriptorList::empty(),
        is_one_of: false,
        specified_by_url: None,
    },
];
static SCHEMA: SchemaDescriptor = SchemaDescriptor {
    abi_version: chigiri::GENERATED_ABI_VERSION,
    schema_digest: chigiri::SchemaDigest::from_bytes([23; 32]),
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

#[derive(Clone, Default)]
pub struct PhaseLog(Arc<Mutex<Vec<&'static str>>>);

impl PhaseLog {
    pub fn push(&self, phase: &'static str) {
        self.0.lock().unwrap().push(phase);
    }
    pub fn snapshot(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Clone)]
pub struct TestContext {
    pub log: PhaseLog,
}

struct Dispatch {
    resolver_calls: Arc<AtomicUsize>,
    pending_drop: Option<Arc<AtomicBool>>,
}

impl ResolverDispatch<TestContext> for Dispatch {
    fn schema_fingerprint(&self) -> chigiri::__private::ResolverSchemaFingerprint {
        chigiri::__private::descriptor_fingerprint(&SCHEMA)
    }

    fn register_custom_scalars(&self, _: &mut CustomScalarRegistry) {}

    fn resolve<'a>(
        &'a self,
        context: &'a TestContext,
        _: &'a str,
        field: &'a str,
        _: Option<&'a ResolvedObject>,
        _: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.resolver_calls.fetch_add(1, Ordering::SeqCst);
        context.log.push("resolver");
        if let Some(dropped) = &self.pending_drop {
            return Box::pin(Pending::new(Arc::clone(dropped)));
        }
        if field == "fail" {
            return Box::pin(async {
                Err(FieldError::public("Synthetic field failure").code("SYNTHETIC_FAILURE"))
            });
        }
        if field == "items" {
            return Box::pin(async {
                Ok(ResolvedValue::List(vec![
                    Ok(ResolvedValue::Leaf(Value::from("first"))),
                    Ok(ResolvedValue::Leaf(Value::from("second"))),
                ]))
            });
        }
        if field == "item" {
            return Box::pin(async { Ok(ResolvedValue::object("Item", ())) });
        }
        Box::pin(async { Ok(ResolvedValue::Leaf(Value::from("ok"))) })
    }
}

struct RecordingPlugin {
    log: PhaseLog,
    reject_operation: bool,
}

impl Plugin<TestContext> for RecordingPlugin {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, TestContext>,
        next: RequestNext<'a, TestContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.log.push("request");
        next.run(context)
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, TestContext>,
        next: OperationNext<'a, TestContext>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.log.push("operation");
        if self.reject_operation {
            Box::pin(async {
                GraphQlResponse::request_error(
                    GraphQlError::new("Operation rejected").code("OPERATION_REJECTED"),
                )
            })
        } else {
            next.run(context)
        }
    }
}

pub fn schema(
    log: PhaseLog,
    resolver_calls: Arc<AtomicUsize>,
    reject_operation: bool,
) -> ExecutableSchema<TestContext> {
    schema_builder(log, resolver_calls, reject_operation)
        .build()
        .unwrap()
}

/// Resets the endpoint test plugin to the built schema's compiled kernel.
pub fn endpoint_builder(
    log: PhaseLog,
    resolver_calls: Arc<AtomicUsize>,
    reject_operation: bool,
) -> MissingContextEndpointBuilder<TestContext> {
    let executable = schema(log.clone(), resolver_calls, reject_operation);
    Endpoint::builder(executable).push_plugin(recording_plugin(log, reject_operation))
}

pub fn recording_plugin(log: PhaseLog, reject_operation: bool) -> impl Plugin<TestContext> {
    RecordingPlugin {
        log,
        reject_operation,
    }
}

pub fn schema_builder(
    log: PhaseLog,
    resolver_calls: Arc<AtomicUsize>,
    reject_operation: bool,
) -> ExecutableSchemaBuilder<TestContext> {
    schema_builder_with_pending(log, resolver_calls, reject_operation, None)
}

pub fn schema_builder_with_pending(
    log: PhaseLog,
    resolver_calls: Arc<AtomicUsize>,
    reject_operation: bool,
    pending_drop: Option<Arc<AtomicBool>>,
) -> ExecutableSchemaBuilder<TestContext> {
    ExecutableSchema::builder(
        &SCHEMA,
        Dispatch {
            resolver_calls,
            pending_drop,
        },
    )
    .plugin(RecordingPlugin {
        log,
        reject_operation,
    })
}

const fn field(name: &'static str) -> FieldDescriptor {
    typed_field(name, "String", STRING)
}

const fn typed_field(
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

pub struct RecordingTransport {
    pub log: PhaseLog,
    pub reject: bool,
}

impl TransportAdmission for RecordingTransport {
    fn admit<'a>(
        &'a self,
        _: TransportRequest<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.log.push("transport");
        let reject = self.reject;
        Box::pin(async move {
            if reject {
                Ok(AdmissionDecision::Reject(rejection(
                    429,
                    "Transport rejected",
                    "RATE_LIMITED",
                )))
            } else {
                Ok(AdmissionDecision::Continue)
            }
        })
    }
}

pub struct RecordingSource {
    pub log: PhaseLog,
    pub reject: bool,
    pub pending_drop: Option<Arc<AtomicBool>>,
}

impl OperationSourceResolver for RecordingSource {
    fn resolve<'a>(
        &'a self,
        envelope: &'a RequestEnvelope,
    ) -> chigiri_http::BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        self.log.push("source");
        if let Some(dropped) = &self.pending_drop {
            return Box::pin(Pending::new(Arc::clone(dropped)));
        }
        let reject = self.reject;
        let document = envelope.document_text().unwrap_or_default().to_owned();
        Box::pin(async move {
            if reject {
                Err(SourceResolutionError::Reject(Box::new(rejection(
                    403,
                    "Source rejected",
                    "SOURCE_REJECTED",
                ))))
            } else {
                Ok(ResolvedOperationSource::new(document))
            }
        })
    }
}

pub struct RecordingDecoded(pub PhaseLog);

impl DecodedRequestHook for RecordingDecoded {
    fn inspect<'a>(
        &'a self,
        _: DecodedRequestView<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.0.push("decoded");
        Box::pin(async { Ok(AdmissionDecision::Continue) })
    }
}

pub struct PendingTransport(pub Arc<AtomicBool>);

impl TransportAdmission for PendingTransport {
    fn admit<'a>(
        &'a self,
        _: TransportRequest<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        Box::pin(Pending::new(Arc::clone(&self.0)))
    }
}

pub struct PendingDecoded(pub Arc<AtomicBool>);

impl DecodedRequestHook for PendingDecoded {
    fn inspect<'a>(
        &'a self,
        _: DecodedRequestView<'a>,
    ) -> chigiri_http::BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        Box::pin(Pending::new(Arc::clone(&self.0)))
    }
}

pub struct Pending<T> {
    dropped: Arc<AtomicBool>,
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Pending<T> {
    pub fn new(dropped: Arc<AtomicBool>) -> Self {
        Self {
            dropped,
            marker: std::marker::PhantomData,
        }
    }
}

impl<T> Future for Pending<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        Poll::Pending
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

pub fn rejection(status: u16, message: &'static str, code: &'static str) -> Rejection {
    Rejection::new(status, GraphQlError::new(message).code(code)).unwrap()
}
