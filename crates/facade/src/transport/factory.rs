//! Shared type erasure for finite and streaming application hooks.

#[cfg(feature = "subscription-worker")]
mod managed;
#[cfg(feature = "subscription-worker")]
pub(crate) use managed::ContextTaskScope;

use std::any::Any;
use std::marker::PhantomData;
use std::sync::Arc;

use chigiri_http::{
    BoxFuture, ContextFactory, HookError, PrepareError, RequestContext, ResponseHeaders,
    ResponseHook, ResponseHookContext,
};
use chigiri_runtime::GraphQlResponse;

pub(crate) trait ErasedContextFactory<C>: Send + Sync {
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<ErasedPreparedContext<C>, PrepareError>>;

    #[cfg(feature = "subscription-worker")]
    fn create_scoped<'a>(
        &'a self,
        request: RequestContext<'a>,
        scope: Option<ContextTaskScope>,
    ) -> BoxFuture<'a, Result<ErasedPreparedContext<C>, PrepareError>>;
}

pub(crate) trait ErasedResponseHook: Send + Sync {
    fn call<'a>(
        &'a self,
        request: RequestContext<'a>,
        response: &'a GraphQlResponse,
        state: &'a (dyn Any + Send + Sync),
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>>;
}

pub(crate) fn erase_factory<C, S, F>(factory: F) -> Arc<dyn ErasedContextFactory<C>>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
    F: ContextFactory<C, S>,
{
    Arc::new(FactoryAdapter {
        factory,
        marker: PhantomData,
    })
}

pub(crate) fn erase_hook<S, H>(hook: H) -> Arc<dyn ErasedResponseHook>
where
    S: Send + Sync + 'static,
    H: ResponseHook<S>,
{
    Arc::new(ResponseHookAdapter {
        hook,
        marker: PhantomData,
    })
}

pub(crate) struct ErasedPreparedContext<C> {
    pub(super) execution_context: C,
    pub(super) response_state: Box<dyn Any + Send + Sync>,
    pub(super) completion: Option<Arc<dyn chigiri_http::ResponseCompletion>>,
}

impl<C> ErasedPreparedContext<C> {
    #[cfg(feature = "subscription-worker")]
    pub(crate) fn take_completion(&mut self) -> Option<Arc<dyn chigiri_http::ResponseCompletion>> {
        self.completion.take()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        C,
        Box<dyn Any + Send + Sync>,
        Option<Arc<dyn chigiri_http::ResponseCompletion>>,
    ) {
        (self.execution_context, self.response_state, self.completion)
    }
}

struct FactoryAdapter<F, S> {
    factory: F,
    marker: PhantomData<fn() -> S>,
}

impl<C, S, F> ErasedContextFactory<C> for FactoryAdapter<F, S>
where
    C: Send + Sync + 'static,
    S: Send + Sync + 'static,
    F: ContextFactory<C, S>,
{
    fn create<'a>(
        &'a self,
        request: RequestContext<'a>,
    ) -> BoxFuture<'a, Result<ErasedPreparedContext<C>, PrepareError>> {
        Box::pin(async move {
            let (execution_context, response_state, completion) =
                self.factory.create(request).await?.__into_parts();
            Ok(ErasedPreparedContext {
                execution_context,
                response_state: Box::new(response_state),
                completion,
            })
        })
    }

    #[cfg(feature = "subscription-worker")]
    fn create_scoped<'a>(
        &'a self,
        request: RequestContext<'a>,
        scope: Option<ContextTaskScope>,
    ) -> BoxFuture<'a, Result<ErasedPreparedContext<C>, PrepareError>> {
        let Some(scope) = scope else {
            return self.create(request);
        };
        Box::pin(async move {
            let prepared =
                managed::ScopedContextFuture::new(self.factory.create(request), scope).await?;
            let (execution_context, response_state, completion) = prepared.__into_parts();
            Ok(ErasedPreparedContext {
                execution_context,
                response_state: Box::new(response_state),
                completion,
            })
        })
    }
}

struct ResponseHookAdapter<H, S> {
    hook: H,
    marker: PhantomData<fn() -> S>,
}

impl<S, H> ErasedResponseHook for ResponseHookAdapter<H, S>
where
    S: Send + Sync + 'static,
    H: ResponseHook<S>,
{
    fn call<'a>(
        &'a self,
        request: RequestContext<'a>,
        response: &'a GraphQlResponse,
        state: &'a (dyn Any + Send + Sync),
    ) -> BoxFuture<'a, Result<ResponseHeaders, HookError>> {
        let state = state
            .downcast_ref::<S>()
            .expect("the builder must align context-factory and response-hook state types");
        self.hook
            .call(ResponseHookContext::__new(request, response, state))
    }
}
