//! Adapter that lets typed editors own trait objects for lower-level contracts.

use std::sync::Arc;

use chigiri_http::{
    AdmissionDecision, AdmissionError, BoxFuture, DecodedRequestHook, DecodedRequestView,
    OperationSourceResolver, RequestEnvelope, ResolvedOperationSource, SourceResolutionError,
    TransportAdmission, TransportRequest,
};
use chigiri_runtime::__private::ResolvedValue;
use chigiri_runtime::{
    ErrorEvent, ErrorId, ErrorIdFactory, ErrorIdFactoryError, ErrorObserver, FieldContext,
    FieldNext, FieldResult, GraphQlResponse, LifecycleEvent, LifecycleObserver, OperationContext,
    OperationNext, Plugin, RequestContext, RequestId, RequestIdFactory, RequestIdFactoryError,
    RequestNext, Sampler, SamplingDecision,
};

pub(crate) struct SharedErrorIds(pub(crate) Arc<dyn ErrorIdFactory>);

impl ErrorIdFactory for SharedErrorIds {
    fn create(&self) -> Result<ErrorId, ErrorIdFactoryError> {
        self.0.create()
    }
}

pub(crate) struct SharedRequestIds(pub(crate) Arc<dyn RequestIdFactory>);

impl RequestIdFactory for SharedRequestIds {
    fn create(&self) -> Result<RequestId, RequestIdFactoryError> {
        self.0.create()
    }
}

pub(crate) struct SharedSampler(pub(crate) Arc<dyn Sampler>);

impl Sampler for SharedSampler {
    fn sample(&self, identity: chigiri_runtime::RequestIdentity<'_>) -> SamplingDecision {
        self.0.sample(identity)
    }
}

pub(crate) struct SharedLifecycle(pub(crate) Arc<dyn LifecycleObserver>);

impl LifecycleObserver for SharedLifecycle {
    fn observe(&self, event: LifecycleEvent<'_>) {
        self.0.observe(event);
    }
}

pub(crate) struct SharedErrors(pub(crate) Arc<dyn ErrorObserver>);

impl ErrorObserver for SharedErrors {
    fn observe_error(&self, event: ErrorEvent<'_>) {
        self.0.observe_error(event);
    }
}

pub(crate) struct SharedPlugin<C>(pub(crate) Arc<dyn Plugin<C>>);

impl<C: 'static> Plugin<C> for SharedPlugin<C> {
    fn request<'a>(
        &'a self,
        context: RequestContext<'a, C>,
        next: RequestNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.0.request(context, next)
    }

    fn operation<'a>(
        &'a self,
        context: OperationContext<'a, C>,
        next: OperationNext<'a, C>,
    ) -> BoxFuture<'a, GraphQlResponse> {
        self.0.operation(context, next)
    }

    fn field<'a>(
        &'a self,
        context: FieldContext<'a, C>,
        next: FieldNext<'a, C>,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.0.field(context, next)
    }
}

pub(crate) struct SharedAdmission(pub(crate) Arc<dyn TransportAdmission>);

impl TransportAdmission for SharedAdmission {
    fn admit<'a>(
        &'a self,
        request: TransportRequest<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.0.admit(request)
    }
}

pub(crate) struct SharedSource(pub(crate) Arc<dyn OperationSourceResolver>);

impl OperationSourceResolver for SharedSource {
    fn resolve<'a>(
        &'a self,
        envelope: &'a RequestEnvelope,
    ) -> BoxFuture<'a, Result<ResolvedOperationSource, SourceResolutionError>> {
        self.0.resolve(envelope)
    }
}

pub(crate) struct SharedDecoded(pub(crate) Arc<dyn DecodedRequestHook>);

impl DecodedRequestHook for SharedDecoded {
    fn inspect<'a>(
        &'a self,
        request: DecodedRequestView<'a>,
    ) -> BoxFuture<'a, Result<AdmissionDecision, AdmissionError>> {
        self.0.inspect(request)
    }
}
