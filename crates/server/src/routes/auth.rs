//! PIN sign-in. Operators with a PIN must present a session token from `/api/login`; operators
//! without one may act by name (convenient during setup).

use std::time::Duration;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{post, put},
    Json, Router,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::{
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    routes::inventory::{User, USER_COLUMNS},
    AppState, CLIENT_HEADER, TOKEN_HEADER,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/users/{id}/pin", put(set_pin))
}

pub fn hash_pin(pin: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .expect("argon2 hashing cannot fail with default params")
        .to_string()
}

pub fn verify_pin(pin: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| {
            Argon2::default()
                .verify_password(pin.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false)
}

fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit())
}

#[derive(Debug, Deserialize)]
pub struct Login {
    name: String,
    #[serde(default)]
    pin: String,
}

#[derive(Serialize)]
pub struct LoginResult {
    token: String,
    user: User,
}

async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(l): Json<Login>,
) -> ApiResult<Json<LoginResult>> {
    let row: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT id, pin_hash FROM users WHERE name = ?")
            .bind(l.name.trim())
            .fetch_optional(&s.pool)
            .await?;
    let ok = match &row {
        Some((_, Some(hash))) => verify_pin(&l.pin, hash),
        Some((_, None)) => true,
        None => false,
    };
    if !ok {
        // Slow down guessing.
        tokio::time::sleep(Duration::from_millis(400)).await;
        return Err(ApiError::Unauthorized("wrong name or PIN".into()));
    }
    let (user_id, _) = row.expect("checked above");
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let client = headers
        .get(CLIENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    sqlx::query("INSERT INTO sessions (token, user_id, client_id, created_at) VALUES (?, ?, ?, ?)")
        .bind(&token)
        .bind(user_id)
        .bind(&client)
        .bind(now())
        .execute(&s.pool)
        .await?;
    let user = sqlx::query_as::<_, User>(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?"))
        .bind(user_id)
        .fetch_one(&s.pool)
        .await?;
    Ok(Json(LoginResult { token, user }))
}

async fn logout(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<StatusCode> {
    if let Some(t) = headers.get(TOKEN_HEADER).and_then(|v| v.to_str().ok()) {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(t)
            .execute(&s.pool)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct SetPin {
    /// `null` removes the PIN.
    pin: Option<String>,
}

/// Admins can set anyone's PIN; operators can set their own.
async fn set_pin(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<SetPin>,
) -> ApiResult<Json<User>> {
    let target =
        sqlx::query_as::<_, User>(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?"))
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or(ApiError::NotFound)?;
    if !actor.is_admin() && actor.name != target.name {
        return Err(ApiError::Forbidden(
            "you can only change your own PIN".into(),
        ));
    }
    if let Some(p) = &b.pin {
        if !valid_pin(p) {
            return Err(ApiError::bad("PIN must be 4–8 digits"));
        }
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE users SET pin_hash = ? WHERE id = ?")
        .bind(b.pin.as_deref().map(hash_pin))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // Changing a PIN signs the operator out everywhere.
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(
        &mut tx,
        &actor,
        "user.pin_changed",
        &serde_json::json!({ "user_id": id, "name": target.name, "has_pin": b.pin.is_some() }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(User {
        has_pin: b.pin.is_some(),
        ..target
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_hash_roundtrip() {
        let h = hash_pin("4821");
        assert!(verify_pin("4821", &h));
        assert!(!verify_pin("4822", &h));
        assert!(!verify_pin("4821", "garbage"));
    }

    #[test]
    fn pin_format() {
        assert!(valid_pin("1234"));
        assert!(valid_pin("12345678"));
        assert!(!valid_pin("123"));
        assert!(!valid_pin("12a4"));
    }
}
