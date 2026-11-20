pub(crate) mod inventory;
mod live;
pub(crate) mod ops;
pub(crate) mod shows;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .merge(inventory::router())
        .merge(shows::router())
        .merge(ops::router())
        .merge(live::router())
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}
