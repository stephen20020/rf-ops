use std::{net::SocketAddr, path::PathBuf, time::Duration};

use rfops_server::{admin_exists, app_with_web, backup, db, seed, upsert_admin, AppState};
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

    // A standby Take over (or a restore) left a verified download to boot into.
    let restored = match db::file_path(&db_url) {
        Some(file) => restore_marker(&file)?,
        None => None,
    };

    let (pool, mut startup_note) = match db::open_verified(&db_url, backup_dir.as_deref()).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(problem = %e, "cannot open the database");
            return Err(e.into());
        }
    };
    if let Some(from) = restored {
        // New epoch: every client discards its snapshot and replays its acknowledged commands.
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('server.epoch', '2')
             ON CONFLICT(key) DO UPDATE SET value = CAST(value AS INTEGER) + 1",
        )
        .execute(&pool)
        .await?;
        startup_note = Some(format!("Restored from {from} at startup (takeover/restore)."));
    }
    if let Some(n) = &startup_note {
        tracing::warn!(note = %n, "database recovered at startup");
    }
    // `rfops-server --create-admin "Name" 123456`: create or reset a Lead, then exit.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--create-admin") {
        let (Some(name), Some(pin)) = (args.get(i + 1), args.get(i + 2)) else {
            return Err("usage: rfops-server --create-admin <name> <pin>".into());
        };
        upsert_admin(&pool, name, pin).await?;
        println!("Lead '{name}' can now sign in with that PIN.");
        return Ok(());
    }
    if seed_demo && seed::seed_demo(&pool).await? {
        tracing::info!("demo data inserted");
    }
    if seed_hf2026 && seed::seed_hf2026(&pool).await? {
        tracing::info!("HF 2026 template inserted");
    }

    // After seeding (seeds create named operators without PINs). On first start the Lead comes
    // from RFOPS_ADMIN_NAME / RFOPS_ADMIN_PIN; there is no "anyone can act" mode.
    if !admin_exists(&pool).await? {
        match (std::env::var("RFOPS_ADMIN_NAME"), std::env::var("RFOPS_ADMIN_PIN")) {
            (Ok(name), Ok(pin)) => {
                upsert_admin(&pool, &name, &pin).await?;
                tracing::info!(%name, "first Lead created from RFOPS_ADMIN_NAME / RFOPS_ADMIN_PIN");
            }
            _ => tracing::warn!(
                "no Lead with a PIN exists: nobody can sign in. Set RFOPS_ADMIN_NAME and RFOPS_ADMIN_PIN, or run `rfops-server --create-admin <name> <pin>`"
            ),
        }
    }
    let mut state = AppState::new(pool.clone())
        .await?
        .with_startup_note(startup_note);
    state.exit_after_takeover = true;
    // Production: R2 off-site backups and the single-writer lease (docs/reliability.md §3.6).
    if let Some(r2) = rfops_server::r2::R2::from_env() {
        let instance = std::env::var("RFOPS_INSTANCE").unwrap_or_else(|_| "active".into());
        let was_holder = rfops_server::midi::setting(&pool, "lease.held_by")
            .await?
            .is_some_and(|h| h == instance);
        let lease =
            rfops_server::lease::LeaseHandle::start_with(r2.clone(), &instance, was_holder).await;
        // Remember whether this server holds the lease, for a restart while R2 is unreachable.
        {
            let (pool, lease, instance) = (pool.clone(), lease.clone(), instance.clone());
            tokio::spawn(async move {
                let mut last = None;
                loop {
                    let held = !lease.is_fenced();
                    if last != Some(held) {
                        let v = held.then_some(instance.as_str());
                        if rfops_server::midi::set_setting(&pool, "lease.held_by", v).await.is_ok() {
                            last = Some(held);
                        }
                    }
                    tokio::time::sleep(rfops_server::lease::RENEW_EVERY).await;
                }
            });
        }
        let st = lease.snapshot();
        tracing::info!(%instance, fenced = st.fenced, reason = %st.reason, "R2 lease");
        state = state
            .with_offsite(backup::r2_offsite(r2, Some(lease.clone())))
            .with_lease(lease);
    } else {
        tracing::warn!("R2 not configured (RFOPS_R2_*): no off-site backups and no standby lease");
    }
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

/// If `<db>.restore-on-boot` exists, move the database aside and put the staged download in its
/// place. Returns the staged file's name. The download was already verified with quick_check.
fn restore_marker(db: &std::path::Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let marker = PathBuf::from(format!("{}.restore-on-boot", db.display()));
    let Ok(staged) = std::fs::read_to_string(&marker) else {
        return Ok(None);
    };
    let staged = PathBuf::from(staged.trim());
    if !staged.exists() {
        std::fs::remove_file(&marker)?;
        tracing::error!(file = %staged.display(), "restore marker points at a missing file; ignored");
        return Ok(None);
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", db.display()));
        if from.exists() {
            std::fs::rename(&from, format!("{}.pre-restore-{stamp}{suffix}", db.display()))?;
        }
    }
    std::fs::rename(&staged, db)?;
    std::fs::remove_file(&marker)?;
    Ok(Some(
        staged
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    ))
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
