#![forbid(unsafe_code)]

use chigiri::ShutdownOptions;
use chigiri_subscriptions_example::{domain::State, endpoint, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let graphql = endpoint(State::default())?;
    let app = router(graphql.clone())?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4000").await?;
    println!("GraphQL: http://{}/graphql", listener.local_addr()?);
    // A no-hook minimum is axum::serve(listener, app).await?. It has no
    // coordinated SDK drain; last service-owner release requests cancellation.
    axum::serve(listener, app)
        .with_graceful_shutdown(graphql.shutdown_on(
            async {
                if let Err(error) = tokio::signal::ctrl_c().await {
                    eprintln!("Shutdown signal failed: {error}");
                }
            },
            ShutdownOptions::graceful(),
        ))
        .await?;
    println!("SDK shutdown: {:?}", graphql.shutdown_outcome());
    Ok(())
}
