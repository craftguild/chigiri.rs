//! Application-owned native HTTP upgrades; the SDK owns GraphQL socket work.

use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

use hyper_util::rt::TokioIo;

use super::{accept, announce, bind, debug_error, join_connection, scoped_stop, wait_for_stop};

pub(super) async fn run_hyper(
    endpoint: chigiri::Endpoint<()>,
    stop: Arc<AtomicBool>,
    started: mpsc::SyncSender<Result<SocketAddr, String>>,
) -> Result<(), String> {
    let observer = endpoint.clone();
    let setup = async move {
        let service = chigiri_hyper::managed_service(endpoint).map_err(debug_error)?;
        let (listener, address) = bind().await?;
        Ok::<_, String>((service, listener, address))
    }
    .await;
    let (service, listener) = announce(setup, started)?;
    let (stream, _) = accept(&listener).await?;
    let upgrade = service.clone();
    let connection = tokio::spawn(async move {
        let builder = hyper::server::conn::http1::Builder::new();
        let http = builder
            .serve_connection(TokioIo::new(stream), upgrade)
            .with_upgrades();
        http.await.map_err(debug_error)
    });
    wait_for_stop(&stop).await;
    let sdk_result = scoped_stop(&observer).await;
    drop(service);
    let native_result = join_connection(connection).await;
    sdk_result.and(native_result)
}

pub(super) async fn run_axum(
    endpoint: chigiri::Endpoint<()>,
    stop: Arc<AtomicBool>,
    started: mpsc::SyncSender<Result<SocketAddr, String>>,
) -> Result<(), String> {
    let observer = endpoint.clone();
    let setup = async move {
        let service = axum::Router::new().route_service(
            "/graphql",
            chigiri_axum::service(endpoint).map_err(debug_error)?,
        );
        let (listener, address) = bind().await?;
        Ok::<_, String>((service, listener, address))
    }
    .await;
    let (service, listener) = announce(setup, started)?;
    let (stream, _) = accept(&listener).await?;
    let upgrade = service.clone();
    let connection = tokio::spawn(async move {
        let builder = hyper::server::conn::http1::Builder::new();
        let upgrade = hyper_util::service::TowerToHyperService::new(upgrade);
        let http = builder
            .serve_connection(TokioIo::new(stream), upgrade)
            .with_upgrades();
        http.await.map_err(debug_error)
    });
    wait_for_stop(&stop).await;
    let sdk_result = scoped_stop(&observer).await;
    drop(service);
    let native_result = join_connection(connection).await;
    sdk_result.and(native_result)
}
