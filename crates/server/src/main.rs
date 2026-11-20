use std::{net::SocketAddr, path::PathBuf};

use rfops_server::{app_with_web, db, seed, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let seed_demo = std::env::args().any(|a| a == "--seed-demo");
    let db_url = std::env::var("RFOPS_DB").unwrap_or_else(|_| "sqlite://rfops.db".into());
    let addr: SocketAddr = std::env::var("RFOPS_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    let web_dir = std::env::var("RFOPS_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/dist"));
    let web_dir = web_dir.join("index.html").exists().then_some(web_dir);

    let pool = db::connect(&db_url).await?;
    if seed_demo && seed::seed_demo(&pool).await? {
        tracing::info!("demo data inserted");
    }
    let state = AppState::new(pool).await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    match &web_dir {
        Some(d) => {
            tracing::info!(%addr, db = %db_url, web = %d.display(), "RF Ops server listening")
        }
        None => {
            tracing::warn!(%addr, db = %db_url, "RF Ops server listening (API only: web/dist not found, build the web client or set RFOPS_WEB)")
        }
    }
    axum::serve(listener, app_with_web(state, web_dir))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
