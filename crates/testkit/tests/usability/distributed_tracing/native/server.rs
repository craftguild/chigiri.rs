//! Ordinary native listeners and SDK stop hooks, without telemetry globals.

use std::net::SocketAddr;
use std::time::Duration;

use axum::middleware::{Next, from_fn};
use hyper::service::{Service, service_fn};
use hyper_util::rt::TokioIo;
use opentelemetry::Context;
use opentelemetry::context::FutureExt;
use opentelemetry::trace::TraceContextExt;

use super::{PrivateValue, parent};

#[derive(Clone, Copy, Debug)]
pub(super) enum Bridge {
    Axum,
    Hyper,
}

#[derive(Clone, Copy)]
enum ConnectionEnd {
    Complete,
    Disconnect,
    FailedBody,
}

pub(super) struct Server {
    pub(super) address: SocketAddr,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub(super) async fn start(
        endpoint: chigiri::Endpoint<()>,
        bridge: Bridge,
        current: bool,
        suppressed: bool,
    ) -> Self {
        Self::start_inner(
            endpoint,
            bridge,
            current,
            suppressed,
            ConnectionEnd::Complete,
        )
        .await
    }

    pub(super) async fn start_streaming(
        endpoint: chigiri::Endpoint<()>,
        bridge: Bridge,
        current: bool,
        suppressed: bool,
    ) -> Self {
        Self::start_inner(
            endpoint,
            bridge,
            current,
            suppressed,
            ConnectionEnd::Disconnect,
        )
        .await
    }

    pub(super) async fn start_failed_stream(
        endpoint: chigiri::Endpoint<()>,
        bridge: Bridge,
        current: bool,
        suppressed: bool,
    ) -> Self {
        Self::start_inner(
            endpoint,
            bridge,
            current,
            suppressed,
            ConnectionEnd::FailedBody,
        )
        .await
    }

    async fn start_inner(
        endpoint: chigiri::Endpoint<()>,
        bridge: Bridge,
        current: bool,
        suppressed: bool,
        connection_end: ConnectionEnd,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let entry = if current {
            Context::new().with_remote_span_context(parent(7))
        } else {
            Context::new()
        };
        let entry = entry.with_value(PrivateValue);
        let entry = if suppressed {
            entry.with_telemetry_suppressed()
        } else {
            entry
        };
        let (stop, receive) = tokio::sync::oneshot::channel();
        let mut hook = endpoint.shutdown_on(
            async {
                let _ = receive.await;
            },
            chigiri::ShutdownOptions::graceful(),
        );
        let task = match bridge {
            Bridge::Axum => {
                let router = axum::Router::new()
                    .route_service("/graphql", chigiri_axum::service(endpoint).unwrap())
                    .layer(from_fn(
                        move |request: axum::extract::Request, next: Next| {
                            let context = entry.clone();
                            async move { next.run(request).await }.with_context(context)
                        },
                    ));
                tokio::spawn(async move {
                    axum::serve(listener, router)
                        .with_graceful_shutdown(hook)
                        .await
                        .unwrap();
                })
            }
            Bridge::Hyper => {
                let graphql = chigiri_hyper::managed_service(endpoint).unwrap();
                tokio::spawn(async move {
                    let mut connections = tokio::task::JoinSet::new();
                    loop {
                        tokio::select! {
                            biased;
                            _ = &mut hook => break,
                            result = connections.join_next(), if !connections.is_empty() => { result.unwrap().unwrap(); }
                            accepted = listener.accept() => {
                                let (stream, _) = accepted.unwrap();
                                let graphql = graphql.clone();
                                let context = entry.clone();
                                connections.spawn(async move {
                                    let service = service_fn(move |request| {
                                        let graphql = graphql.clone();
                                        let context = context.clone();
                                        async move { graphql.call(request).await }.with_context(context)
                                    });
                                    let result = hyper::server::conn::http1::Builder::new()
                                        .serve_connection(TokioIo::new(stream), service).with_upgrades().await;
                                    if let Err(error) = result {
                                        let expected = match connection_end {
                                            ConnectionEnd::Complete => false,
                                            ConnectionEnd::Disconnect => error.is_incomplete_message(),
                                            ConnectionEnd::FailedBody => error.is_user(),
                                        };
                                        assert!(expected, "{error:?}");
                                    }
                                });
                            }
                        }
                    }
                    while let Some(result) = connections.join_next().await {
                        result.unwrap();
                    }
                })
            }
        };
        Self {
            address,
            stop,
            task,
        }
    }

    pub(super) async fn stop(self) {
        self.stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}
