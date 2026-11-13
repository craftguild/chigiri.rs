//! Test-only fallible service for isolated native transport boundaries.

use std::sync::Arc;

use chigiri::__streaming::{
    HttpStreamingDriver, HttpStreamingError, HttpStreamingRegistration, PreparedHttpSubscription,
    SubscriptionHttpIngress, SubscriptionRuntimeGuard,
};
use chigiri::Endpoint;
use chigiri_http::{ApplicationMetadata, BoxFuture, RequestHeader, TraceId, TransportRequest};
use hyper::{Request, Response, body::Incoming, service::Service};

use super::{HyperStreamingBody, response};
use crate::{PeerAddr, body::HyperBody, deadline::TokioDeadline};

static DEADLINES: TokioDeadline = TokioDeadline;

/// Test-only service retaining the explicit pre-head failure boundary.
pub struct HyperStreamingService<C> {
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

impl<C> Clone for HyperStreamingService<C> {
    fn clone(&self) -> Self {
        Self {
            endpoint: self.endpoint.clone(),
            registration: Arc::clone(&self.registration),
        }
    }
}

impl<C> HyperStreamingService<C> {
    /// Closes fixture registration while retained clones exercise cleanup.
    pub fn close(&self) {
        self.registration.inner.close();
    }
}

impl<C: Send + Sync + 'static> HyperStreamingService<C> {
    /// Creates the capacity-one upgrade fixture for a single test connection.
    pub fn websocket_connection(
        &self,
    ) -> (
        super::websocket::HyperWebSocketUpgradeService<C>,
        super::websocket::HyperWebSocketConnectionDriver<C>,
    ) {
        super::websocket::connection(self.endpoint.clone())
    }
}

/// Creates explicit owners only for internal transport and recovery tests.
pub fn streaming_service<C>(
    endpoint: Endpoint<C>,
) -> Result<
    (
        HyperStreamingService<C>,
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
        HyperStreamingService {
            endpoint,
            registration: Arc::new(ServiceRegistration {
                inner: registration,
            }),
        },
        driver,
        guard,
    ))
}

impl<C> Service<Request<Incoming>> for HyperStreamingService<C>
where
    C: Send + Sync + 'static,
{
    type Response = Response<HyperStreamingBody>;
    type Error = HttpStreamingError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let endpoint = self.endpoint.clone();
        let registration = Arc::clone(&self.registration);
        Box::pin(async move { handle(endpoint, &registration.inner, request).await })
    }
}

async fn handle<C>(
    endpoint: Endpoint<C>,
    registration: &HttpStreamingRegistration,
    request: Request<Incoming>,
) -> Result<Response<HyperStreamingBody>, HttpStreamingError>
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
    let peer = parts.extensions.get::<PeerAddr>().map(|peer| peer.0);
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
        .__handle_http_streaming(transport, HyperBody(body), &DEADLINES)
        .await
    {
        SubscriptionHttpIngress::Finite(parts) => Ok(response::finite(parts)),
        SubscriptionHttpIngress::Pending(pending) => match pending.prepare().await {
            PreparedHttpSubscription::Failed(_) => Err(HttpStreamingError::Worker),
            PreparedHttpSubscription::Response(response) => Ok(response::prepared(response)),
            PreparedHttpSubscription::Ready(ready) => {
                registration.attach(ready).map(response::streaming)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::sync::Arc;

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
            "hyper-streaming-close.graphql",
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

    fn assert_hyper_service<S>()
    where
        S: Service<
                Request<Incoming>,
                Response = Response<HyperStreamingBody>,
                Error = HttpStreamingError,
            > + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
    {
    }

    #[test]
    fn service_is_a_fallible_hyper_service() {
        assert_hyper_service::<HyperStreamingService<()>>();
    }

    #[tokio::test]
    async fn explicit_close_drains_the_registration_while_a_clone_is_retained() {
        let (service, driver, guard) = streaming_service(endpoint()).unwrap();
        let retained = service.clone();
        let mut driver = Box::pin(driver);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(driver.as_mut().poll(&mut context).is_pending());

        service.close();
        retained.close();
        driver.await.unwrap();

        drop(retained);
        drop(service);
        guard.shutdown().unwrap().await.unwrap();
    }
}
