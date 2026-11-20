use std::{net::SocketAddr, path::PathBuf, time::Duration};

use rfops_server::{app_with_web, backup, db, seed, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let seed_demo = std::env::args().any(|a| a == "--seed-demo");
    let seed_hf2026 = std::env::args().any(|a| a == "--seed-hf2026");
    let db_url = std::env::var("RFOPS_DB").unwrap_or_else(|_| "sqlite://rfops.db".into());
    // Render and most hosts provide PORT; RFOPS_ADDR overrides.
    let addr: SocketAddr = match std::env::var("RFOPS_ADDR") {
        Ok(a) => a.parse()?,
        Err(_) => format!(
            "0.0.0.0:{}",
            std::env::var("PORT").unwrap_or_else(|_| "8080".into())
        )
        .parse()?,
    };
    let web_dir = std::env::var("RFOPS_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/dist"));
    let web_dir = web_dir.join("index.html").exists().then_some(web_dir);

    // Backups go next to the database file unless RFOPS_BACKUP_DIR says otherwise.
    let backup_dir = db::file_path(&db_url).map(|file| {
        std::env::var("RFOPS_BACKUP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                file.parent()
                    .map(|p| p.join("backups"))
                    .unwrap_or_else(|| PathBuf::from("backups"))
            })
    });

    let (pool, startup_note) = match db::open_verified(&db_url, backup_dir.as_deref()).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(problem = %e, "cannot open the database");
            return Err(e.into());
        }
    };
    if let Some(n) = &startup_note {
        tracing::warn!(note = %n, "database recovered at startup");
    }
    if seed_demo && seed::seed_demo(&pool).await? {
        tracing::info!("demo data inserted");
    }
    if seed_hf2026 && seed::seed_hf2026(&pool).await? {
        tracing::info!("HF 2026 template inserted");
    }

    let mut state = AppState::new(pool.clone())
        .await?
        .with_startup_note(startup_note);
    if let Some(dir) = backup_dir {
        tracing::info!(dir = %dir.display(), "verified backups: every 2 min in show mode, 15 min otherwise, and at show start");
        state = state.with_backups(dir, backup::SHOW_INTERVAL, backup::IDLE_INTERVAL);
    }

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
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    // Leave the database tidy: flush the WAL into the main file before exiting.
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool),
    )
    .await;
    pool.close().await;
    tracing::info!("RF Ops server stopped cleanly");
    Ok(())
}

/// Ctrl-C, or SIGTERM from launchd / Render / systemd. In-flight requests finish; open
/// WebSockets are dropped and clients reconnect to the next instance.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("shutdown signal received; finishing in-flight requests");
}
