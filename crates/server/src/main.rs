use std::net::SocketAddr;

use rfops_server::{app, db, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let db_url = std::env::var("RFOPS_DB").unwrap_or_else(|_| "sqlite://rfops.db".into());
    let addr: SocketAddr = std::env::var("RFOPS_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;

    let pool = db::connect(&db_url).await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, db = %db_url, "RF Ops server listening");
    axum::serve(listener, app(AppState { pool }))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
