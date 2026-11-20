//! Standby takeover (docs/reliability.md §3.6, runbook step 3).
//!
//! `POST /api/standby/takeover {name, pin}` on a fenced service:
//! 1. downloads the newest verified backup (highest lease generation) from R2;
//! 2. runs quick_check on it;
//! 3. checks the Lead's name and PIN against *that backup's* users (this disk may be empty);
//! 4. takes the lease (conditional write, generation + 1);
//! 5. leaves a restore marker and restarts, so the server boots into the restored database with a
//!    new epoch (clients resync and replay their acknowledged commands).

use std::path::PathBuf;

use axum::{extract::State, http::StatusCode, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    backup,
    error::{ApiError, ApiResult},
    routes::auth::verify_pin,
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/standby/status", get(status))
        .route("/api/standby/takeover", post(takeover))
}

/// The file whose presence tells the next start to restore a downloaded backup.
pub fn marker_for(db_file: &std::path::Path) -> PathBuf {
    PathBuf::from(format!("{}.restore-on-boot", db_file.display()))
}

/// Public: what Take over would restore. The runbook shows the backup's age; older than 15 min is
/// beyond the clients' replay window and gets a warning on the takeover screen.
async fn status(State(s): State<AppState>) -> Json<Value> {
    let Some(l) = &s.lease else {
        return Json(json!({ "lease": null, "fenced": false, "r2": false }));
    };
    let newest = match l.r2.list("").await {
        Ok(items) => {
            let keys: Vec<String> = items.iter().map(|o| o.key.clone()).collect();
            backup::pick_newest(&keys).map(|k| {
                let modified = items
                    .iter()
                    .find(|o| o.key == k)
                    .map(|o| o.last_modified.clone())
                    .unwrap_or_default();
                let age = chrono::DateTime::parse_from_rfc3339(&modified)
                    .ok()
                    .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds());
                json!({ "key": k, "last_modified": modified, "age_s": age })
            })
        }
        Err(e) => Some(json!({ "error": e.to_string() })),
    };
    Json(json!({ "lease": l.snapshot(), "fenced": l.is_fenced(), "r2": true, "newest_backup": newest }))
}

#[derive(Deserialize)]
pub struct Takeover {
    name: String,
    pin: String,
}

async fn db_file(s: &AppState) -> ApiResult<PathBuf> {
    let p: Option<String> =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_optional(&s.pool)
            .await?;
    p.filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| ApiError::bad("in-memory database: nothing to restore into"))
}

async fn takeover(
    State(s): State<AppState>,
    Json(b): Json<Takeover>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let lease = s
        .lease
        .clone()
        .ok_or_else(|| ApiError::bad("no R2 configured: takeover needs the R2 backups"))?;
    if !lease.is_fenced() {
        return Err(ApiError::Conflict(
            "this server is already the active one".into(),
        ));
    }
    // Same throttle as sign-in: a public endpoint that checks a PIN.
    let rules = [crate::auth::Rule::new("takeover", 5, 300, 300)];
    if let Some(wait) = s.auth.limiter.blocked(&rules) {
        return Err(ApiError::TooMany(format!(
            "too many wrong PINs; try again in {} min",
            wait.div_ceil(60)
        )));
    }

    // 1. Newest backup of the highest generation.
    let keys: Vec<String> = lease
        .r2
        .list("")
        .await
        .map_err(|e| ApiError::bad(format!("cannot list backups in R2: {e}")))?
        .into_iter()
        .map(|o| o.key)
        .collect();
    let key = backup::pick_newest(&keys)
        .ok_or_else(|| ApiError::bad("no backups found in R2; nothing to take over with"))?;
    let (bytes, _) = lease
        .r2
        .get(&key)
        .await
        .map_err(|e| ApiError::bad(format!("download failed: {e}")))?
        .ok_or_else(|| ApiError::bad("the backup disappeared while downloading"))?;
    let live = db_file(&s).await?;
    let staged = PathBuf::from(format!("{}.takeover.db", live.display()));
    tokio::fs::write(&staged, &bytes)
        .await
        .map_err(|e| ApiError::bad(format!("cannot write the download: {e}")))?;

    // 2. Integrity.
    if let Err(e) = crate::db::quick_check_file(&staged).await {
        let _ = std::fs::remove_file(&staged);
        return Err(ApiError::bad(format!("backup {key} failed its check: {e}")));
    }

    // 3. The Lead's PIN, as stored in the backup.
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&staged)
        .read_only(true);
    let snap = sqlx::SqlitePool::connect_with(opts).await?;
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT role, pin_hash FROM users WHERE name = ? COLLATE NOCASE")
            .bind(b.name.trim())
            .fetch_optional(&snap)
            .await?;
    snap.close().await;
    let ok = matches!(&row, Some((role, Some(h))) if role == "admin" && verify_pin(&b.pin, h));
    if !ok {
        s.auth.limiter.fail(&rules);
        let _ = std::fs::remove_file(&staged);
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        return Err(ApiError::Unauthorized(
            "only a Lead (name + PIN as of the latest backup) can take over".into(),
        ));
    }

    // 4. The lease.
    let generation = lease.take_over().await.map_err(ApiError::Conflict)?;

    // 5. Restore on restart.
    tokio::fs::write(marker_for(&live), staged.to_string_lossy().as_bytes())
        .await
        .map_err(|e| ApiError::bad(format!("cannot write the restore marker: {e}")))?;
    tracing::warn!(%key, generation, by = %b.name, "TAKEOVER: lease taken; restarting into the restored backup");
    if s.exit_after_takeover {
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // Render (or launchd/systemd) restarts the process; main restores the marker.
            std::process::exit(3);
        });
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "restored_from": key,
            "generation": generation,
            "next": "The server restarts in a few seconds. Then point the Cloudflare Worker ORIGIN at this service.",
        })),
    ))
}
