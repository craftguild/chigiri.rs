//! Expected finite execution timeout does not fail the managed endpoint.

use chigiri_runtime::__private::{
    ArgumentValues, ErasedSubscription, ErasedSubscriptionArguments, ErasedSubscriptionEvent,
    ResolvedObject, ResolvedValue, ResolverDispatch, ResolverSchemaFingerprint,
    SubscriptionResolverDispatch,
};
use chigiri_runtime::core::executable::CustomScalarRegistry;
use chigiri_runtime::{FieldContext, FieldResult};

use super::*;

struct PendingExecution(Dispatch);
impl ResolverDispatch<AppContext> for PendingExecution {
    fn schema_fingerprint(&self) -> ResolverSchemaFingerprint {
        self.0.schema_fingerprint()
    }
    fn register_custom_scalars(&self, registry: &mut CustomScalarRegistry) {
        self.0.register_custom_scalars(registry);
    }
    fn resolve<'a>(
        &'a self,
        context: &'a AppContext,
        parent_type: &'a str,
        field_name: &'a str,
        parent: Option<&'a ResolvedObject>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        if matches!(parent_type, "Query" | "Mutation") && field_name == "ping" {
            let probe = self.0.0.clone();
            Box::pin(async move {
                probe.counts.nested_calls.fetch_add(1, Ordering::SeqCst);
                std::future::pending().await
            })
        } else {
            self.0
                .resolve(context, parent_type, field_name, parent, arguments)
        }
    }
}
impl SubscriptionResolverDispatch<AppContext> for PendingExecution {
    fn subscription_root(&self) -> Option<&str> {
        self.0.subscription_root()
    }
    fn subscribe<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: ArgumentValues,
    ) -> BoxFuture<'a, FieldResult<ErasedSubscription>> {
        self.0.subscribe(context, arguments)
    }
    fn resolve_event<'a>(
        &'a self,
        context: FieldContext<'a, AppContext>,
        arguments: &'a ErasedSubscriptionArguments,
        event: &'a ErasedSubscriptionEvent,
    ) -> BoxFuture<'a, FieldResult<ResolvedValue>> {
        self.0.resolve_event(context, arguments, event)
    }
}

#[test]
fn expected_query_and_mutation_execution_timeout_keeps_the_endpoint_usable() {
    let runtime = runtime();
    for document in ["query { ping }", "mutation { ping }"] {
        let probe = State::new(Vec::new());
        let errors = Arc::new(AtomicUsize::new(0));
        let schema = ExecutableSchema::builder_with_subscriptions(
            &SCHEMA,
            PendingExecution(Dispatch(probe.clone())),
        )
        .build()
        .unwrap();
        let endpoint = Endpoint::builder(schema)
            .context_factory(Factory(probe.clone()))
            .response_hook(Hook(probe.clone()))
            .inject(|components| {
                components
                    .http()
                    .execution_timeout(Duration::from_millis(10));
                components
                    .observability()
                    .error_observer(Errors(errors.clone()));
            })
            .build()
            .unwrap();
        run(&runtime, async {
            let managed = managed_socket::open(&endpoint).await;
            managed.peer.subscribe("expired", document);
            until(&probe.counts.nested_calls, 1).await;
            tokio::time::timeout(Duration::from_secs(2), async {
                while !managed
                    .peer
                    .messages()
                    .iter()
                    .any(|frame| frame.contains("REQUEST_TIMEOUT"))
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the existing execution deadline must produce a safe timeout");
            assert_eq!(endpoint.shutdown_outcome(), None);
            assert_eq!(count(&errors), 0);
            assert!(managed.service.request().is_ok());
            managed.peer.subscribe("after", "query { __typename }");
            tokio::time::timeout(Duration::from_secs(2), async {
                while !managed.peer.messages().iter().any(|frame| {
                    frame.contains(r#""id":"after""#) && frame.contains(r#""__typename":"Query""#)
                }) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("another finite operation must still return its actual data");
            endpoint
                .shutdown_on(async {}, ShutdownOptions::graceful())
                .await;
            assert_eq!(endpoint.shutdown_outcome(), Some(ShutdownOutcome::Drained));
            assert_eq!(count(&probe.counts.factory_calls), 2);
            assert_eq!(count(&probe.counts.completion_calls), 0);
            assert_eq!(count(&errors), 0);
        });
    }
}
