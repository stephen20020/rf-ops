use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Forbidden(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl ApiError {
    pub fn bad(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m.clone()),
            ApiError::Db(sqlx::Error::RowNotFound) => (StatusCode::NOT_FOUND, "not found".into()),
            ApiError::Db(e) => {
                if let Some(db) = e.as_database_error() {
                    if db.is_unique_violation() {
                        return (
                            StatusCode::CONFLICT,
                            Json(json!({ "error": "already exists" })),
                        )
                            .into_response();
                    }
                    if db.is_foreign_key_violation() {
                        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "referenced record does not exist or is still in use" })))
                            .into_response();
                    }
                }
                tracing::error!(error = %e, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
