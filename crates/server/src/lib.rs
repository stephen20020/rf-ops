pub mod auth;
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

/// Create the first Lead or reset a Lead's PIN (env `RFOPS_ADMIN_*` or `--create-admin`).
pub async fn upsert_admin(pool: &SqlitePool, name: &str, pin: &str) -> Result<i64, String> {
    routes::auth::upsert_admin(pool, name, pin).await
}

pub async fn admin_exists(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    routes::auth::admin_exists(pool).await
}
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
    pub auth: Arc<auth::AuthConfig>,
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
        let key = auth::load_key(&pool).await?;
        let origin = std::env::var("RFOPS_PUBLIC_ORIGIN")
            .ok()
            .filter(|o| !o.is_empty());
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
            auth: Arc::new(auth::AuthConfig::new(key, origin)),
        })
    }

    /// TESTS ONLY: take identity from the legacy `x-rfops-actor` header with no session, as
    /// before step 1b. The server binary never calls this; `tests/security.rs` checks the
    /// default rejects header identity.
    #[doc(hidden)]
    pub async fn insecure_header_auth_for_tests(mut self) -> Self {
        let key = auth::load_key(&self.pool).await.expect("session key");
        let mut cfg = auth::AuthConfig::new(key, None);
        cfg.header_auth = true;
        self.auth = Arc::new(cfg);
        self
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
        let _ = sqlx::query("DELETE FROM session_revocations WHERE expires_at < ?")
            .bind(auth::unix_now())
            .execute(&pool)
            .await;
        let _ = sqlx::query("DELETE FROM join_codes WHERE expires_at < ? OR uses_left <= 0")
            .bind(auth::unix_now())
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
        ))
        // Outermost on the API: unauthenticated requests never reach the idempotency store.
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));
    if let Some(dir) = web_dir {
        // The app shell (code only, no show data) is public so the sign-in screen can load.
        let index = dir.join("index.html");
        router = router.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }
    router
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::security_headers,
        ))
        .with_state(state)
}

pub const ACTOR_HEADER: &str = "x-rfops-actor";
pub const CLIENT_HEADER: &str = "x-rfops-client";
pub const IDEMPOTENCY_HEADER: &str = "x-rfops-idempotency-key";

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
        if !state.auth.header_auth {
            let p = parts
                .extensions
                .get::<auth::Principal>()
                .cloned()
                .ok_or_else(|| ApiError::Unauthorized("sign in required".into()))?;
            if p.role == "viewer" {
                return Err(ApiError::Forbidden("viewers cannot make changes".into()));
            }
            if p.kind == auth::Kind::User && !p.has_pin && p.role != "helper" {
                return Err(ApiError::Forbidden(
                    "set a PIN before making changes; ask the Lead".into(),
                ));
            }
            let name = match (p.kind, header(auth::ON_BEHALF_HEADER)) {
                // "Wall (Sam)": only names of real operators are accepted.
                (auth::Kind::Screen, Some(who)) => {
                    let known: Option<String> =
                        sqlx::query_scalar("SELECT name FROM users WHERE name = ?")
                            .bind(&who)
                            .fetch_optional(&state.pool)
                            .await?;
                    match known {
                        Some(n) => format!("{} ({n})", p.name),
                        None => p.name.clone(),
                    }
                }
                _ => p.name.clone(),
            };
            return Ok(Actor {
                name,
                client_id: header(CLIENT_HEADER),
                role: Some(p.role),
            });
        }

        // Legacy header identity (tests only, see `insecure_header_auth_for_tests`).
        let name = header(ACTOR_HEADER)
            .ok_or_else(|| ApiError::bad(format!("missing {ACTOR_HEADER} header")))?;
        let any_users: Option<i64> = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
            .fetch_optional(&state.pool)
            .await?;
        let role = if any_users.is_some() {
            let row: Option<(String,)> = sqlx::query_as("SELECT role FROM users WHERE name = ?")
                .bind(&name)
                .fetch_optional(&state.pool)
                .await?;
            let (role,) =
                row.ok_or_else(|| ApiError::Forbidden(format!("unknown operator '{name}'")))?;
            if role == "viewer" {
                return Err(ApiError::Forbidden("viewers cannot make changes".into()));
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
