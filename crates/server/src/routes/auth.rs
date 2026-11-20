//! Sign-in endpoints (docs/hosting-and-security.md §Security 2–6).
//!
//! - Leads, A2s and FOH sign in with name + PIN. There is no name-only sign-in.
//! - Helpers sign in with a 6-digit join code: per named helper, up to 3 devices, 2 hours.
//! - Wall and FOH screens are enrolled by a Lead and get a 7-day sliding screen credential.

use std::time::Duration;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Extension, Json, Router,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;

use crate::{
    auth::{self, Claims, Kind, Principal, Rule},
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    routes::inventory::{User, USER_COLUMNS},
    AppState,
};

/// Join codes: uses per code and lifetime.
pub const JOIN_USES: i64 = 3;
pub const JOIN_TTL: i64 = 2 * 3600;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/auth/status", get(status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/join", post(join))
        .route("/api/auth/logout", post(logout))
        .route(
            "/api/auth/join-codes",
            post(create_join_code).delete(reissue_all_codes),
        )
        .route("/api/auth/screens", get(list_screens).post(enrol_screen))
        .route("/api/auth/screens/{id}", delete(revoke_screen))
        .route("/api/users/{id}/pin", put(set_pin))
        .route("/api/users/{id}/sign-out", post(sign_out_user))
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

/// At most two argon2 checks run at once, so PIN guessing can't starve the server of CPU.
fn argon_permits() -> &'static tokio::sync::Semaphore {
    static S: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(2))
}

async fn verify_pin_throttled(pin: String, hash: Option<String>) -> bool {
    let _permit = argon_permits().acquire().await.expect("semaphore never closed");
    tokio::task::spawn_blocking(move || match hash {
        Some(h) => verify_pin(&pin, &h),
        None => {
            // Same work for unknown names, so timing doesn't reveal who exists.
            let _ = verify_pin(&pin, dummy_hash());
            false
        }
    })
    .await
    .unwrap_or(false)
}

/// An argon2 hash of a random value, used to equalise timing for unknown names.
fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| hash_pin(&auth::random_hex(16)))
}

/// Name of the long-lived cookie marking a device that has signed in as someone before
/// (OWASP "device cookie"): such devices keep their own small budget when a name is locked.
pub const DEVICE_COOKIE: &str = "rfops_device";

fn device_cookie_for(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == DEVICE_COOKIE)
        .map(|(_, v)| v.to_string())
}

fn device_cookie_value(s: &AppState, lname: &str) -> String {
    let nonce = auth::random_hex(8);
    format!("{nonce}.{}", &s.auth.keyed_hash("device", &format!("{lname}\0{nonce}"))[..32])
}

fn trusted_device(s: &AppState, headers: &HeaderMap, lname: &str) -> bool {
    device_cookie_for(headers)
        .and_then(|v| {
            let (nonce, mac) = v.split_once('.')?;
            Some(s.auth.keyed_hash("device", &format!("{lname}\0{nonce}"))[..32] == *mac)
        })
        .unwrap_or(false)
}

pub fn valid_pin(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit())
}

/// Create the first Lead, or reset that Lead's PIN. Used by `RFOPS_ADMIN_NAME` /
/// `RFOPS_ADMIN_PIN` on first start and by `rfops-server --create-admin`.
pub async fn upsert_admin(pool: &SqlitePool, name: &str, pin: &str) -> Result<i64, String> {
    if name.trim().is_empty() {
        return Err("admin name is empty".into());
    }
    if !valid_pin(pin) {
        return Err("admin PIN must be 4–8 digits".into());
    }
    let hash = hash_pin(pin);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO users (name, role, pin_hash) VALUES (?, 'admin', ?)
         ON CONFLICT(name) DO UPDATE SET role = 'admin', pin_hash = excluded.pin_hash,
           sessions_valid_after = ?
         RETURNING id",
    )
    .bind(name.trim())
    .bind(&hash)
    .bind(now())
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(id)
}

/// True when some Lead can sign in (has a PIN).
pub async fn admin_exists(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users WHERE role = 'admin' AND pin_hash IS NOT NULL)",
    )
    .fetch_one(pool)
    .await
}

fn with_cookie<T: Serialize>(cookie: header::HeaderValue, body: T) -> Response {
    let mut r = Json(body).into_response();
    r.headers_mut().append(header::SET_COOKIE, cookie);
    r
}

fn require_admin(actor: &Actor, what: &str) -> ApiResult<()> {
    if actor.is_admin() {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("only the Lead can {what}")))
    }
}

/// Raise a security WARNING for the Lead (once per key while open).
async fn security_alert(pool: &SqlitePool, dedupe: &str, title: &str, detail: &str) {
    let r = sqlx::query(
        "INSERT INTO alerts (show_id, severity, source, title, detail, dedupe_key, opened_at, opened_by)
         SELECT NULL, 'warning', 'rule', ?, ?, ?, ?, 'system'
         WHERE NOT EXISTS (SELECT 1 FROM alerts WHERE dedupe_key = ? AND state <> 'resolved')",
    )
    .bind(title)
    .bind(detail)
    .bind(dedupe)
    .bind(now())
    .bind(dedupe)
    .execute(pool)
    .await;
    match r {
        Ok(r) if r.rows_affected() > 0 => {
            if let Ok(mut tx) = pool.begin().await {
                let sys = Actor {
                    name: "system".into(),
                    client_id: None,
                    role: Some("admin".into()),
                };
                let _ = log_event(
                    &mut tx,
                    &sys,
                    "alert.opened",
                    &json!({ "dedupe_key": dedupe, "title": title }),
                )
                .await;
                let _ = tx.commit().await;
            }
            tracing::warn!(%title, %detail, "security alert");
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, "could not record security alert"),
    }
}

/// Count a failure from this IP; above 100 in 5 min the Lead is warned (never a lockout,
/// because the whole crew shares the venue's public IP).
async fn note_ip_failure(s: &AppState, headers: &HeaderMap) {
    if let Some(ip) = auth::client_ip(headers) {
        let r = [Rule::new(format!("ip:{ip}"), 100, 300, 300)];
        if !s.auth.limiter.fail(&r).is_empty() {
            security_alert(
                &s.pool,
                &format!("auth:ip:{ip}"),
                "Many failed sign-ins",
                &format!("More than 100 failed sign-ins in 5 minutes from {ip}. Check nobody is guessing PINs."),
            )
            .await;
        }
    }
}

// ---------------------------------------------------------------- status

async fn status(State(s): State<AppState>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    let me = auth::authenticate(&s, &headers).await.ok().map(|(p, _)| p);
    Ok(Json(json!({
        "signed_in": me,
        "admin_exists": admin_exists(&s.pool).await?,
    })))
}

// ---------------------------------------------------------------- PIN sign-in

#[derive(Debug, Deserialize)]
pub struct Login {
    name: String,
    #[serde(default)]
    pin: String,
}

async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(l): Json<Login>,
) -> ApiResult<Response> {
    let name = l.name.trim().to_string();
    let lname = name.to_lowercase();
    let dev = auth::client_id(&headers).unwrap_or_else(|| "no-device".into());
    let trusted = trusted_device(&s, &headers, &lname);
    let mut rules = vec![
        // 5 wrong PINs lock that name on that device for 5 minutes.
        Rule::new(format!("login:{lname}|{dev}"), 5, 300, 300),
    ];
    if trusted {
        // A device that signed in as this name before keeps its own budget, so an attacker
        // locking the name can't lock the real person out.
        rules.push(Rule::new(format!("login:{lname}|trusted"), 10, 3600, 900));
    } else {
        // Across all other devices: 20 an hour pauses the name for 15 minutes. Device ids are
        // chosen by the client, so this stops guessing by rotating them.
        rules.push(Rule::new(format!("login:{lname}"), 20, 3600, 900));
    }
    if let Some(wait) = s.auth.limiter.blocked(&rules) {
        return Err(ApiError::TooMany(format!(
            "too many wrong PINs; try again in {} min",
            wait.div_ceil(60)
        )));
    }
    let row: Option<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT id, role, pin_hash FROM users WHERE name = ? COLLATE NOCASE")
            .bind(&name)
            .fetch_optional(&s.pool)
            .await?;
    if let Some((_, role, None)) = &row {
        return Err(ApiError::Forbidden(if role == "helper" {
            "helpers sign in with a join code from the Lead".into()
        } else {
            "no PIN set yet; ask the Lead to set your PIN".into()
        }));
    }
    let hash = row.as_ref().and_then(|(_, _, h)| h.clone());
    let ok = verify_pin_throttled(l.pin.clone(), hash).await;
    if !ok {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let locked = s.auth.limiter.fail(&rules);
        note_ip_failure(&s, &headers).await;
        if locked.iter().any(|k| !k.contains('|')) {
            security_alert(
                &s.pool,
                &format!("auth:name:{lname}"),
                "Sign-ins paused for an operator",
                &format!("20 wrong PINs for '{name}' in an hour from unknown devices. Sign-in for that name is paused for 15 minutes on new devices; devices that signed in before still work."),
            )
            .await;
        }
        let mut tx = s.pool.begin().await?;
        let anon = Actor {
            name: name.clone(),
            client_id: Some(dev),
            role: None,
        };
        log_event(&mut tx, &anon, "auth.failed", &json!({ "name": name })).await?;
        tx.commit().await?;
        return Err(ApiError::Unauthorized("wrong name or PIN".into()));
    }
    let (user_id, _, _) = row.expect("checked above");
    s.auth.limiter.clear(&format!("login:{lname}|{dev}"));
    let user = sqlx::query_as::<_, User>(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?"))
        .bind(user_id)
        .fetch_one(&s.pool)
        .await?;
    let mut tx = s.pool.begin().await?;
    let who = Actor {
        name: user.name.clone(),
        client_id: Some(dev),
        role: Some(user.role.clone()),
    };
    log_event(&mut tx, &who, "auth.signed_in", &json!({ "user_id": user.id })).await?;
    tx.commit().await?;
    let claims = Claims::new(Kind::User, user_id);
    let mut r = with_cookie(s.auth.cookie(&claims), json!({ "user": user }));
    let secure = if s.auth.public_origin.as_deref().is_some_and(|o| o.starts_with("https://")) {
        "; Secure"
    } else {
        ""
    };
    if let Ok(v) = header::HeaderValue::from_str(&format!(
        "{DEVICE_COOKIE}={}; Path=/api/auth; HttpOnly; SameSite=Strict; Max-Age=31536000{secure}",
        device_cookie_value(&s, &lname)
    )) {
        r.headers_mut().append(header::SET_COOKIE, v);
    }
    Ok(r)
}

/// Sign-in data changed (PIN, codes, revocations): copy it off the server now, so a standby
/// restore doesn't bring back a revoked session or a used-up code.
fn backup_soon(s: &AppState) {
    let s = s.clone();
    tokio::spawn(async move {
        if let Some(Err(e)) = s.backup_now("auto-auth").await {
            tracing::warn!(error = %e, "backup after sign-in change failed");
        }
    });
}

// ---------------------------------------------------------------- join codes (helpers)

#[derive(Debug, Deserialize)]
pub struct NewJoinCode {
    user_id: i64,
}

async fn create_join_code(
    State(s): State<AppState>,
    actor: Actor,
    Json(b): Json<NewJoinCode>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    require_admin(&actor, "create join codes")?;
    let (name, role): (String, String) =
        sqlx::query_as("SELECT name, role FROM users WHERE id = ?")
            .bind(b.user_id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or(ApiError::NotFound)?;
    if role != "helper" {
        return Err(ApiError::bad(
            "join codes are for helpers; other operators sign in with a PIN",
        ));
    }
    let code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000));
    let expires = auth::unix_now() + JOIN_TTL;
    let mut tx = s.pool.begin().await?;
    // One live code per helper: a new one replaces the old.
    sqlx::query("DELETE FROM join_codes WHERE user_id = ?")
        .bind(b.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO join_codes (code_hash, user_id, uses_left, expires_at, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(s.auth.keyed_hash("join", &code))
    .bind(b.user_id)
    .bind(JOIN_USES)
    .bind(expires)
    .bind(&actor.name)
    .bind(now())
    .execute(&mut *tx)
    .await?;
    log_event(
        &mut tx,
        &actor,
        "auth.join_code_created",
        &json!({ "user_id": b.user_id, "name": name }),
    )
    .await?;
    tx.commit().await?;
    backup_soon(&s);
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "code": code,
            "name": name,
            "uses": JOIN_USES,
            "expires_at": chrono::DateTime::from_timestamp(expires, 0).map(|t| t.to_rfc3339()),
        })),
    ))
}

async fn reissue_all_codes(State(s): State<AppState>, actor: Actor) -> ApiResult<StatusCode> {
    require_admin(&actor, "cancel join codes")?;
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("DELETE FROM join_codes")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    log_event(&mut tx, &actor, "auth.join_codes_cancelled", &json!({ "count": n })).await?;
    tx.commit().await?;
    backup_soon(&s);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct Join {
    code: String,
}

async fn join(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(j): Json<Join>,
) -> ApiResult<Response> {
    let dev = auth::client_id(&headers).unwrap_or_else(|| "no-device".into());
    let rules = [
        Rule::new(format!("join|{dev}"), 5, 300, 300),
        // A global cap stops guessing by rotating device ids: 20 wrong codes in 5 minutes
        // pauses joining for 15 minutes (with at most a few codes live, ~1 M combinations).
        Rule::new("join", 20, 300, 900),
    ];
    if let Some(wait) = s.auth.limiter.blocked(&rules) {
        return Err(ApiError::TooMany(format!(
            "too many wrong codes; try again in {} min",
            wait.div_ceil(60)
        )));
    }
    let code: String = j.code.chars().filter(char::is_ascii_digit).collect();
    let hash = s.auth.keyed_hash("join", &code);
    let mut tx = s.pool.begin().await?;
    let row: Option<(i64,)> = sqlx::query_as(
        "UPDATE join_codes SET uses_left = uses_left - 1
         WHERE code_hash = ? AND uses_left > 0 AND expires_at > ?
         RETURNING user_id",
    )
    .bind(&hash)
    .bind(auth::unix_now())
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id,)) = row else {
        tx.rollback().await?;
        tokio::time::sleep(Duration::from_millis(400)).await;
        if s.auth.limiter.fail(&rules).iter().any(|k| k == "join") {
            security_alert(
                &s.pool,
                "auth:join",
                "Join codes paused",
                "20 wrong join codes in 5 minutes. Joining is paused for 15 minutes; give helpers a moment, or sign them in from the Lead's device.",
            )
            .await;
        }
        note_ip_failure(&s, &headers).await;
        return Err(ApiError::Unauthorized(
            "that code is wrong, used up or expired; ask the Lead for a new one".into(),
        ));
    };
    let user = sqlx::query_as::<_, User>(&format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?"))
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
    let who = Actor {
        name: user.name.clone(),
        client_id: Some(dev),
        role: Some(user.role.clone()),
    };
    log_event(&mut tx, &who, "auth.joined", &json!({ "user_id": user.id })).await?;
    tx.commit().await?;
    backup_soon(&s);
    let claims = Claims::new(Kind::User, user_id);
    Ok(with_cookie(s.auth.cookie(&claims), json!({ "user": user })))
}

// ---------------------------------------------------------------- sign out, revoke

async fn revoke_sid(pool: &SqlitePool, sid: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO session_revocations (sid, expires_at) VALUES (?, ?)")
        .bind(sid)
        .bind(auth::unix_now() + auth::SCREEN_TTL + 86_400)
        .execute(pool)
        .await?;
    Ok(())
}

async fn logout(
    State(s): State<AppState>,
    principal: Option<Extension<Principal>>,
) -> ApiResult<Response> {
    if let Some(Extension(p)) = principal {
        revoke_sid(&s.pool, &p.sid).await?;
        backup_soon(&s);
    }
    let mut r = StatusCode::NO_CONTENT.into_response();
    r.headers_mut()
        .append(header::SET_COOKIE, s.auth.clear_cookie());
    Ok(r)
}

/// Sign an operator out on every device.
async fn sign_out_user(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    require_admin(&actor, "sign operators out")?;
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("UPDATE users SET sessions_valid_after = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    sqlx::query("DELETE FROM join_codes WHERE user_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "auth.user_signed_out", &json!({ "user_id": id })).await?;
    tx.commit().await?;
    backup_soon(&s);
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- screens

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Screen {
    id: i64,
    name: String,
    role: String,
    enrolled_by: String,
    enrolled_at: String,
    last_seen_at: Option<String>,
    revoked: bool,
}

async fn list_screens(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<Vec<Screen>>> {
    require_admin(&actor, "see enrolled screens")?;
    Ok(Json(
        sqlx::query_as::<_, Screen>(
            "SELECT id, name, role, enrolled_by, enrolled_at, last_seen_at, revoked <> 0 AS revoked
             FROM screens ORDER BY name",
        )
        .fetch_all(&s.pool)
        .await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct Enrol {
    name: String,
    #[serde(default = "default_screen_role")]
    role: String,
}

fn default_screen_role() -> String {
    "a2".into()
}

/// Turn the browser the Lead is using into an enrolled screen. The Lead's own session on this
/// browser is replaced by the screen credential.
async fn enrol_screen(
    State(s): State<AppState>,
    actor: Actor,
    principal: Option<Extension<Principal>>,
    Json(b): Json<Enrol>,
) -> ApiResult<Response> {
    require_admin(&actor, "enrol screens")?;
    if !["a2", "foh", "viewer"].contains(&b.role.as_str()) {
        return Err(ApiError::bad("screen role must be a2, foh or viewer"));
    }
    let name = b.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad("screen name is required"));
    }
    let mut tx = s.pool.begin().await?;
    // Re-enrolling a name (new device for "Wall SL") replaces the old credential.
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO screens (name, role, enrolled_by, enrolled_at) VALUES (?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET role = excluded.role, enrolled_by = excluded.enrolled_by,
           enrolled_at = excluded.enrolled_at, revoked = 0
         RETURNING id",
    )
    .bind(name)
    .bind(&b.role)
    .bind(&actor.name)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    log_event(
        &mut tx,
        &actor,
        "auth.screen_enrolled",
        &json!({ "screen_id": id, "name": name, "role": b.role }),
    )
    .await?;
    tx.commit().await?;
    backup_soon(&s);
    if let Some(Extension(p)) = principal {
        revoke_sid(&s.pool, &p.sid).await?;
    }
    let claims = Claims::new(Kind::Screen, id);
    Ok(with_cookie(
        s.auth.cookie(&claims),
        json!({ "screen": { "id": id, "name": name, "role": b.role } }),
    ))
}

async fn revoke_screen(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    require_admin(&actor, "remove screens")?;
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("UPDATE screens SET revoked = 1 WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    log_event(&mut tx, &actor, "auth.screen_revoked", &json!({ "screen_id": id })).await?;
    tx.commit().await?;
    backup_soon(&s);
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- PINs

#[derive(Debug, Deserialize)]
pub struct SetPin {
    /// `null` removes the PIN (helpers only).
    pin: Option<String>,
}

/// The Lead can set anyone's PIN; operators can set their own. Changing a PIN signs that
/// operator out everywhere.
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
    match &b.pin {
        Some(p) if !valid_pin(p) => return Err(ApiError::bad("PIN must be 4–8 digits")),
        None if target.role != "helper" => {
            return Err(ApiError::bad(
                "every operator except helpers needs a PIN",
            ))
        }
        _ => {}
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE users SET pin_hash = ?, sessions_valid_after = ? WHERE id = ?")
        .bind(b.pin.as_deref().map(hash_pin))
        .bind(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(
        &mut tx,
        &actor,
        "user.pin_changed",
        &json!({ "user_id": id, "name": target.name, "has_pin": b.pin.is_some() }),
    )
    .await?;
    tx.commit().await?;
    backup_soon(&s);
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
