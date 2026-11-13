//! Own one native connection and retain its original body failure through join.

use chigiri::Endpoint;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub(super) fn start(
    hyper: bool,
    graphql: Endpoint<()>,
    listener: TcpListener,
    hook: chigiri::BoxFuture<'static, ()>,
) -> JoinHandle<Result<(), hyper::Error>> {
    if hyper {
        let service = chigiri_hyper::managed_service(graphql).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let result = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service.clone())
                .await;
            hook.await;
            drop(service);
            result
        })
    } else {
        let app =
            axum::Router::new().route_service("/graphql", chigiri_axum::service(graphql).unwrap());
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let result = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), TowerToHyperService::new(app.clone()))
                .await;
            hook.await;
            drop(app);
            result
        })
    }
}
