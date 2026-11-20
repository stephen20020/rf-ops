pub mod backup;
pub mod db;
pub mod error;
pub mod hub;
pub mod midi;
mod routes;
pub mod rules;
pub mod seed;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{extract::FromRequestParts, http::request::Parts, middleware, Router};
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

pub use db::Actor;
use error::ApiError;
use hub::Hub;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub hub: Arc<Hub>,
    pub midi: midi::Shared,
    /// Where backups go; `None` disables backups (in-memory databases).
    pub backup_dir: Option<PathBuf>,
    pub backup_status: backup::SharedStatus,
    /// Off-site copy of each verified backup (Cloudflare R2 in production).
    pub offsite: Option<backup::Offsite>,
    pub started: Instant,
    /// Increases whenever a standby takes over, so clients know to resync fully.
    pub epoch: i64,
    pub presence: hub::Presence,
    /// Set when startup had to recover the database; shown on the health page.
    pub startup_note: Option<String>,
}

impl AppState {
    /// Creates the state and starts the event broadcaster and rule engine. Must be called inside
    /// a Tokio runtime.
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        let hub = Hub::start(pool.clone()).await?;
        tokio::spawn(rules::run(
            pool.clone(),
            hub.clone(),
            Duration::from_secs(15),
        ));
        midi::ensure_key(&pool).await?;
        let midi_state: midi::Shared = Arc::new(std::sync::Mutex::new(midi::MidiState::new()));
        tokio::spawn(midi::watchdog(
            pool.clone(),
            midi_state.clone(),
            hub.clone(),
        ));
        let epoch = ensure_epoch(&pool).await?;
        tokio::spawn(housekeeping(pool.clone()));
        Ok(Self {
            pool,
            hub,
            midi: midi_state,
            backup_dir: None,
            backup_status: Arc::new(Mutex::new(backup::BackupStatus::default())),
            offsite: None,
            started: Instant::now(),
            epoch,
            presence: hub::Presence::default(),
            startup_note: None,
        })
    }

    /// Enable verified backups to `dir`: every `show_every` while a show is in show mode, every
    /// `idle_every` otherwise, plus at show start and on demand.
    pub fn with_backups(
        mut self,
        dir: PathBuf,
        show_every: Duration,
        idle_every: Duration,
    ) -> Self {
        tokio::spawn(backup::periodic(
            self.pool.clone(),
            dir.clone(),
            self.backup_status.clone(),
            self.offsite.clone(),
            show_every,
            idle_every,
        ));
        self.backup_dir = Some(dir);
        self
    }

    /// Copy every verified backup off the machine (call before `with_backups`).
    pub fn with_offsite(mut self, offsite: backup::Offsite) -> Self {
        self.offsite = Some(offsite);
        self
    }

    pub fn with_startup_note(mut self, note: Option<String>) -> Self {
        self.startup_note = note;
        self
    }

    /// Take a backup now (show start, manual). No-op without a backup directory.
    pub async fn backup_now(&self, reason: &str) -> Option<Result<backup::BackupFile, String>> {
        let dir = self.backup_dir.as_ref()?;
        Some(
            backup::backup_and_ship(
                &self.pool,
                dir,
                reason,
                &self.backup_status,
                self.offsite.as_ref(),
            )
            .await,
        )
    }
}

async fn ensure_epoch(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let v: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'server.epoch'")
            .fetch_optional(pool)
            .await?;
    match v.and_then(|v| v.parse().ok()) {
        Some(e) => Ok(e),
        None => {
            sqlx::query(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('server.epoch', '1')",
            )
            .execute(pool)
            .await?;
            Ok(1)
        }
    }
}

/// Hourly: prune idempotency keys and expired sessions, and re-run the integrity check.
async fn housekeeping(pool: SqlitePool) {
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
        let day_ago = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
        let _ = sqlx::query("DELETE FROM idempotency WHERE created_at < ?")
            .bind(&day_ago)
            .execute(&pool)
            .await;
        if let Err(e) = db::quick_check(&pool).await {
            tracing::error!(problem = %e, "hourly integrity check FAILED; restore from backup after the show");
        }
    }
}

/// API router. With `web_dir`, also serves the built web client (SPA fallback to index.html).
pub fn app(state: AppState) -> Router {
    app_with_web(state, None)
}

pub fn app_with_web(state: AppState, web_dir: Option<PathBuf>) -> Router {
    let mut router = routes::router()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            hub::idempotency,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            hub::notify_on_write,
        ));
    if let Some(dir) = web_dir {
        let index = dir.join("index.html");
        router = router.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }
    router.with_state(state)
}

pub const ACTOR_HEADER: &str = "x-rfops-actor";
pub const CLIENT_HEADER: &str = "x-rfops-client";
pub const IDEMPOTENCY_HEADER: &str = "x-rfops-idempotency-key";
pub const TOKEN_HEADER: &str = "x-rfops-token";

impl FromRequestParts<AppState> for Actor {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = |name: &str| {
            parts
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let name = header(ACTOR_HEADER)
            .ok_or_else(|| ApiError::bad(format!("missing {ACTOR_HEADER} header")))?;

        // Bootstrap mode: until the first user exists, any name may act (as admin).
        let any_users: Option<i64> = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
            .fetch_optional(&state.pool)
            .await?;
        let role = if any_users.is_some() {
            let row: Option<(i64, String, bool)> =
                sqlx::query_as("SELECT id, role, pin_hash IS NOT NULL FROM users WHERE name = ?")
                    .bind(&name)
                    .fetch_optional(&state.pool)
                    .await?;
            let (user_id, role, has_pin) =
                row.ok_or_else(|| ApiError::Forbidden(format!("unknown operator '{name}'")))?;
            if role == "viewer" {
                return Err(ApiError::Forbidden("viewers cannot make changes".into()));
            }
            if has_pin {
                let token = header(TOKEN_HEADER).unwrap_or_default();
                let ok: Option<i64> = sqlx::query_scalar(
                    "SELECT user_id FROM sessions WHERE token = ? AND user_id = ?",
                )
                .bind(&token)
                .bind(user_id)
                .fetch_optional(&state.pool)
                .await?;
                if ok.is_none() {
                    return Err(ApiError::Unauthorized(format!(
                        "PIN sign-in required for {name}"
                    )));
                }
            }
            Some(role)
        } else {
            None
        };
        Ok(Actor {
            name,
            client_id: header(CLIENT_HEADER),
            role,
        })
    }
}

/// Deserialize helper for PATCH bodies: absent => None, null => Some(None), value => Some(Some(v)).
pub(crate) mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(d).map(Some)
    }
}
