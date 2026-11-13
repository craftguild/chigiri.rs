//! Test-only fallible service for isolated native transport boundaries.

use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes, HttpBody};
use axum::extract::ConnectInfo;
use axum::http::{Request, Response};
use chigiri::__streaming::{
    HttpStreamingDriver, HttpStreamingError, HttpStreamingRegistration, PreparedHttpSubscription,
    ReadyHttpSubscription, SubscriptionHttpIngress, SubscriptionRuntimeGuard,
    SupervisedHttpResponse,
};
use chigiri::Endpoint;
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use tower::Service;

use super::response;
use crate::{body::AxumBody, deadline::TokioDeadline};

static DEADLINES: TokioDeadline = TokioDeadline;

/// Test-only service retaining the explicit pre-head failure boundary.
pub struct AxumStreamingService<C> {
    endpoint: Endpoint<C>,
    registration: Arc<ServiceRegistration>,
}

struct ServiceRegistration {
    inner: HttpStreamingRegistration,
}

impl Drop for ServiceRegistration {
    fn drop(&mut self) {
        self.inner.close();
    }
}

impl<C> Clone for AxumStreamingService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            registration: Arc::clone(&self.registration),
        }
    }
}

impl<C> AxumStreamingService<C> {
    /// Closes fixture registration while retained clones exercise cleanup.
    pub fn close(&self) {
        self.registration.inner.close();
    }
}

impl<C: Send + Sync + 'static> AxumStreamingService<C> {
    /// Creates the capacity-one upgrade fixture for a single test connection.
    pub fn websocket_connection(
        &self,
    ) -> (
        super::websocket::AxumWebSocketUpgradeService<C>,
        super::websocket::AxumWebSocketConnectionDriver<C>,
    ) {
        super::websocket::connection(self.endpoint.clone())
    }
}

/// Creates explicit owners only for internal transport and recovery tests.
pub fn streaming_service<C>(
    endpoint: Endpoint<C>,
) -> Result<
    (
        AxumStreamingService<C>,
        HttpStreamingDriver,
        SubscriptionRuntimeGuard,
    ),
    HttpStreamingError,
>
where
    C: Send + Sync + 'static,
{
    let guard = endpoint
        .__register_subscription_runtime()
        .map_err(HttpStreamingError::Resource)?;
    let (registration, driver) = endpoint.__http_streaming_supervision()?;
    Ok((
        AxumStreamingService {
            endpoint,
            registration: Arc::new(ServiceRegistration {
                inner: registration,
            }),
        },
        driver,
        guard,
    ))
}

impl<C, B> Service<Request<B>> for AxumStreamingService<C>
where
    C: Send + Sync + 'static,
    B: HttpBody<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    type Response = Response<Body>;
    type Error = HttpStreamingError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<B>) -> Self::Future {
        let endpoint = self.endpoint.clone();
        let registration = Arc::clone(&self.registration);
        let request = request.map(Body::new);
        Box::pin(async move { handle(endpoint, &registration.inner, request).await })
    }
}

async fn handle<C>(
    endpoint: Endpoint<C>,
    registration: &HttpStreamingRegistration,
    request: Request<Body>,
) -> Result<Response<Body>, HttpStreamingError>
where
    C: Send + Sync + 'static,
{
    let (parts, body) = request.into_parts();
    let headers: Vec<_> = parts
        .headers
        .iter()
        .map(|(name, value)| RequestHeader::new(name.as_str(), value.as_bytes()))
        .collect();
    let default_metadata = ApplicationMetadata::default();
    let metadata = parts
        .extensions
        .get::<ApplicationMetadata>()
        .unwrap_or(&default_metadata);
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0);
    let trace_id = parts.extensions.get::<TraceId>();
    let mut transport = TransportRequest::new(
        parts.method.as_str(),
        parts.uri.path(),
        parts.uri.query(),
        &headers,
        peer,
        metadata,
    );
    if let Some(trace_id) = trace_id {
        transport = transport.with_trace_id(trace_id);
    }
    match endpoint
        .__handle_http_streaming(transport, AxumBody(body), &DEADLINES)
        .await
    {
        SubscriptionHttpIngress::Finite(parts) => Ok(crate::response::into_response(parts)),
        SubscriptionHttpIngress::Pending(pending) => {
            prepared_response(pending.prepare().await, |ready| registration.attach(ready))
        }
    }
}

fn prepared_response(
    prepared: PreparedHttpSubscription,
    attach: impl FnOnce(ReadyHttpSubscription) -> Result<SupervisedHttpResponse, HttpStreamingError>,
) -> Result<Response<Body>, HttpStreamingError> {
    match prepared {
        PreparedHttpSubscription::Failed(_) => Err(HttpStreamingError::Worker),
        PreparedHttpSubscription::Response(response) => Ok(response::prepared(response)),
        PreparedHttpSubscription::Ready(ready) => attach(ready).map(response::streaming),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::Arc;

    use chigiri::__streaming::SubscriptionWorkerRuntimeError;
    use chigiri::core::Source;
    use chigiri::core::executable::CustomScalarRegistry;
    use chigiri::core::executor::{ArgumentValues, ResolvedObject, ResolvedValue};
    use chigiri::experimental::{OwnedResolverDispatch, OwnedSchema};
    use chigiri::{BoxFuture, FieldResult, Value};

    struct Dispatch {
        schema: OwnedSchema,
        coordinates: [(Arc<str>, Arc<str>); 1],
    }

    impl OwnedResolverDispatch<()> for Dispatch {
        fn schema(&self) -> &OwnedSchema {
            &self.schema
        }

        fn coordinates(&self) -> &[(Arc<str>, Arc<str>)] {
            &self.coordinates
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
            Box::pin(async { Ok(ResolvedValue::Leaf(Value::Int(1))) })
        }
    }

    fn endpoint() -> Endpoint<()> {
        let schema = OwnedSchema::parse(vec![Source::new(
            "axum-streaming-close.graphql",
            "type Query { value: Int! }",
        )])
        .unwrap();
        let dispatch = Dispatch {
            schema: schema.clone(),
            coordinates: [(Arc::from("Query"), Arc::from("value"))],
        };
        Endpoint::builder(schema.bind(dispatch).unwrap())
            .without_context()
            .build()
            .unwrap()
    }

    fn assert_tower_service<S>()
    where
        S: Service<Request<Body>, Response = Response<Body>, Error = HttpStreamingError>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
    {
    }

    #[test]
    fn service_is_a_fallible_generic_tower_service() {
        assert_tower_service::<AxumStreamingService<()>>();
    }

    #[test]
    fn failed_preparation_returns_an_error_without_attachment() {
        let result = prepared_response(
            PreparedHttpSubscription::Failed(SubscriptionWorkerRuntimeError::Invariant),
            |_| panic!("failed preparation must not attach a response"),
        );

        assert!(matches!(result, Err(HttpStreamingError::Worker)));
    }

    #[tokio::test]
    async fn explicit_close_drains_the_registration_while_a_clone_is_retained() {
        let (service, driver, guard) = streaming_service(endpoint()).unwrap();
        let retained = service.clone();
        let mut driver = Box::pin(driver);
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(driver.as_mut().poll(&mut context).is_pending());

        service.close();
        retained.close();
        driver.await.unwrap();

        drop(retained);
        drop(service);
        guard.shutdown().unwrap().await.unwrap();
    }
}
