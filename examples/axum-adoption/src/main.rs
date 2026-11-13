//! Starts the synthetic Axum adoption server.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address =
        std::env::var("CHIGIRI_AXUM_ADDRESS").unwrap_or_else(|_| "127.0.0.1:4100".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    axum::serve(listener, chigiri_axum_adoption::router()).await?;
    Ok(())
}
