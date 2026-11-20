pub mod db;
pub mod error;
mod routes;

use axum::{extract::FromRequestParts, http::request::Parts, Router};
use sqlx::SqlitePool;

pub use db::Actor;
use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

pub fn app(state: AppState) -> Router {
    routes::router().with_state(state)
}

pub const ACTOR_HEADER: &str = "x-rfops-actor";
pub const CLIENT_HEADER: &str = "x-rfops-client";

impl<S: Send + Sync> FromRequestParts<S> for Actor {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
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
        Ok(Actor {
            name,
            client_id: header(CLIENT_HEADER),
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
