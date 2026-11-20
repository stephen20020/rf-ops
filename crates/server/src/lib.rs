pub mod db;
pub mod error;
pub mod hub;
mod routes;
pub mod seed;

use std::{path::PathBuf, sync::Arc};

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
}

impl AppState {
    /// Creates the state and starts the event broadcaster. Must be called inside a Tokio runtime.
    pub async fn new(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        let hub = Hub::start(pool.clone()).await?;
        Ok(Self { pool, hub })
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
            let role: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE name = ?")
                .bind(&name)
                .fetch_optional(&state.pool)
                .await?;
            let role =
                role.ok_or_else(|| ApiError::Forbidden(format!("unknown operator '{name}'")))?;
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
