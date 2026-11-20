pub(crate) mod auth;
mod data;
pub(crate) mod inventory;
mod live;
pub(crate) mod midi;
pub(crate) mod ops;
mod plan;
mod swaps;
mod showops;
pub(crate) mod shows;
mod standby;

use axum::{extract::State, routing::get, Json, Router};
use serde_json::{json, Value};

use crate::{db, error::ApiResult, hub, Actor, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/health/detail", get(health_detail))
        .route("/api/presence", get(presence))
        .merge(inventory::router())
        .merge(shows::router())
        .merge(ops::router())
        .merge(live::router())
        .merge(auth::router())
        .merge(showops::router())
        .merge(data::router())
        .merge(midi::router())
        .merge(standby::router())
        .merge(plan::router())
        .merge(swaps::router())
}

/// Public liveness probe for Render and uptime monitors: status only, no show data.
async fn health(State(s): State<AppState>) -> Json<Value> {
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&s.pool)
        .await
        .is_ok();
    Json(json!({ "ok": db_ok, "version": env!("CARGO_PKG_VERSION"), "epoch": s.epoch }))
}

fn file_bytes(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

fn age_secs(iso: &Option<String>) -> Option<i64> {
    iso.as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
}

/// Detailed health for operators (reliability req 26).
async fn health_detail(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    if !actor.is_admin() {
        return Err(crate::error::ApiError::Forbidden(
            "health detail is for the Lead".into(),
        ));
    }
    let integrity = db::quick_check(&s.pool).await;
    let db_path: Option<String> =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_optional(&s.pool)
            .await?;
    let db_path = db_path
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from);
    let wal = db_path
        .as_ref()
        .and_then(|p| file_bytes(&std::path::PathBuf::from(format!("{}-wal", p.display()))));
    let size = db_path.as_ref().and_then(|p| file_bytes(p));
    let disk = s
        .backup_dir
        .as_deref()
        .or(db_path.as_deref().and_then(|p| p.parent()))
        .map(crate::backup::disk_state);
    let backup = s
        .backup_status
        .lock()
        .expect("backup status poisoned")
        .clone();
    let seq = hub::max_seq(&s.pool).await?;
    let bridge = s.midi.lock().expect("midi state poisoned").bridge.clone();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "server_time": db::now(),
        "uptime_s": s.started.elapsed().as_secs(),
        "epoch": s.epoch,
        "event_seq": seq,
        "startup_note": s.startup_note,
        "database": {
            "integrity": match &integrity { Ok(()) => "ok".to_string(), Err(e) => e.clone() },
            "bytes": size,
            "wal_bytes": wal,
        },
        "disk": disk,
        "backups": {
            "enabled": s.backup_dir.is_some(),
            "last_ok_at": backup.last_ok_at,
            "last_ok_age_s": age_secs(&backup.last_ok_at),
            "last_error": backup.last_error,
            "offsite_enabled": s.offsite.is_some(),
            "last_offsite_at": backup.last_offsite_at,
            "last_offsite_age_s": age_secs(&backup.last_offsite_at),
            "last_offsite_error": backup.last_offsite_error,
        },
        "screens": s.presence.list(),
        "bridge": bridge,
    })))
}

async fn presence(State(s): State<AppState>) -> Json<Vec<hub::Screen>> {
    Json(s.presence.list())
}
