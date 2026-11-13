//! Verify that both UI bridges apply rate, source, and deadline policy exactly once.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use chigiri::{Endpoint, graphiql::Paths};
use chigiri_operation::{
    CatalogLimits, CatalogRevision, InMemoryOperationCatalog, PersistedOperationResolver,
};

use super::{
    graphiql_wire::{Bridge, Running},
    rate_support::{OperationKey, TransportKey},
    support::{execute, resolvers},
};

const INTROSPECTION: &str = "{__schema{queryType{name}}}";

#[tokio::test]
async fn dx35_18_ui_does_not_consume_shared_transport_or_operation_quota() {
    for operation in [false, true] {
        let mut builder = Endpoint::builder(resolvers())
            .without_context()
            .graphiql(true);
        if operation {
            let stage = chigiri::rate::operation(OperationKey)
                .fixed_window()
                .limit(1)
                .unwrap()
                .window(Duration::from_secs(3600))
                .unwrap()
                .build()
                .unwrap();
            builder = builder.inject(|parts| {
                parts.rate_limit().stage_b(stage);
            });
        } else {
            let stage = chigiri::rate::transport(TransportKey)
                .fixed_window()
                .limit(1)
                .unwrap()
                .window(Duration::from_secs(3600))
                .unwrap()
                .build()
                .unwrap();
            builder = builder.rate_limit(stage);
        }
        let endpoint = builder.build().unwrap();
        for (index, bridge) in [Bridge::Axum, Bridge::Hyper].into_iter().enumerate() {
            let server = Running::spawn(endpoint.clone(), Paths::default(), bridge, false).await;
            for path in ["/graphiql", "/graphiql/assets/app.js"] {
                assert_eq!(server.request("GET", path, "", "").await.status, 200);
            }
            let response = server.graphql("/graphql", "{greeting}", "").await;
            assert_eq!(response.status, if index == 0 { 200 } else { 429 });
            let response = server.graphql("/graphql", INTROSPECTION, "").await;
            // Stage B refuses introspection even at the single cost ceiling before quota consumption.
            assert_eq!(response.status, if operation { 422 } else { 429 });
            assert_eq!(
                response.json()["errors"][0]["extensions"]["code"],
                if operation {
                    "OPERATION_COST_EXCEEDED"
                } else {
                    "RATE_LIMITED"
                }
            );
            server.stop().await;
        }
        if operation {
            assert_eq!(
                execute(&endpoint, "{greeting}").errors[0]
                    .extensions
                    .get("code")
                    .and_then(chigiri::Value::as_str),
                Some("RATE_LIMITED")
            );
        }
    }
}

#[tokio::test]
async fn dx35_18_public_introspection_respects_the_same_safelist() {
    let catalog = InMemoryOperationCatalog::new(
        CatalogRevision::new("graphiql-test").unwrap(),
        vec![],
        CatalogLimits::default(),
    )
    .unwrap();
    let endpoint = Endpoint::builder(resolvers())
        .without_context()
        .graphiql(true)
        .operation_source(PersistedOperationResolver::safelist(catalog))
        .build()
        .unwrap();
    for bridge in [Bridge::Axum, Bridge::Hyper] {
        let server = Running::spawn(endpoint.clone(), Paths::default(), bridge, false).await;
        assert_eq!(server.request("GET", "/graphiql", "", "").await.status, 200);
        for query in ["{greeting}", INTROSPECTION] {
            let response = server.graphql("/graphql", query, "").await;
            assert_eq!(response.status, 403);
            assert_eq!(
                response.json()["errors"][0]["extensions"]["code"],
                "PERSISTED_OPERATION_NOT_ALLOWED"
            );
        }
        server.stop().await;
    }
}

#[tokio::test]
async fn dx35_18_ui_does_not_enter_execution_and_graphql_deadline_drops_pending_work() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let endpoint = Endpoint::builder(resolvers())
        .without_context()
        .graphiql(true)
        .push_plugin(Pending(Arc::clone(&dropped)))
        .execution_timeout(Duration::from_millis(10))
        .build()
        .unwrap();
    for (index, bridge) in [Bridge::Axum, Bridge::Hyper].into_iter().enumerate() {
        let server = Running::spawn(endpoint.clone(), Paths::default(), bridge, false).await;
        assert_eq!(server.request("GET", "/graphiql", "", "").await.status, 200);
        assert_eq!(dropped.load(Ordering::SeqCst), index);
        let response = server.graphql("/graphql", INTROSPECTION, "").await;
        assert_eq!(response.status, 504);
        assert_eq!(
            response.json()["errors"][0]["extensions"]["code"],
            "REQUEST_TIMEOUT"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), index + 1);
        assert_eq!(endpoint.in_flight_requests(), 0);
        server.stop().await;
    }
}

struct Pending(Arc<AtomicUsize>);

impl chigiri::Plugin<()> for Pending {
    fn request<'a>(
        &'a self,
        _: chigiri::RequestContext<'a, ()>,
        _: chigiri::RequestNext<'a, ()>,
    ) -> chigiri::BoxFuture<'a, chigiri::GraphQlResponse> {
        Box::pin(async move {
            let _guard = Dropped(Arc::clone(&self.0));
            std::future::pending().await
        })
    }
}

struct Dropped(Arc<AtomicUsize>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
