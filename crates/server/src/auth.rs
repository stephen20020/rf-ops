//! Cloud sign-in (docs/hosting-and-security.md §Security).
//!
//! Sessions are signed, stateless cookies (HMAC-SHA256 with the session key), so a standby that
//! shares `RFOPS_SESSION_KEY` accepts them after a takeover. What must persist lives in the
//! database: revoked session ids, `users.sessions_valid_after`, enrolled screens and join codes.
//!
//! Every `/api/` request needs a valid session except the few public endpoints in
//! [`is_public`]. State-changing requests and WebSocket upgrades must come from our own origin.

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, Method},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::{error::ApiError, AppState, CLIENT_HEADER};

pub const COOKIE: &str = "rfops_session";
/// Operator sessions: 18 h, sliding.
pub const USER_TTL: i64 = 18 * 3600;
/// Enrolled screens (wall, FOH): 7 days, sliding.
pub const SCREEN_TTL: i64 = 7 * 24 * 3600;
/// A cookie older than this is re-issued with a fresh expiry on the next request.
pub const REISSUE_AFTER: i64 = 3600;
/// Header a wall screen sends when someone tapped their name chip ("Wall (Sam)").
pub const ON_BEHALF_HEADER: &str = "x-rfops-on-behalf";

type HmacSha256 = Hmac<Sha256>;

/// How people identify themselves (`RFOPS_AUTH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// Default: no sign-in. Each device sends the name it picked (`x-rfops-actor`) so actions are
    /// attributed; anyone with the address can act. Viewers named in Admin stay read-only.
    Open,
    /// Name + PIN, helper join codes and enrolled screens (signed session cookies).
    Pin,
}

impl AuthMode {
    pub fn from_env() -> Self {
        match std::env::var("RFOPS_AUTH").as_deref() {
            Ok("pin") => AuthMode::Pin,
            _ => AuthMode::Open,
        }
    }
}

/// Sign-in configuration, shared by every request.
pub struct AuthConfig {
    key: Vec<u8>,
    /// `https://rfops.example.com`; when set, Origin must match it exactly and cookies are Secure.
    pub public_origin: Option<String>,
    pub mode: AuthMode,
    pub limiter: Limiter,
}

impl AuthConfig {
    pub fn new(key: Vec<u8>, public_origin: Option<String>) -> Self {
        Self {
            key,
            public_origin: public_origin.map(|o| o.trim_end_matches('/').to_string()),
            mode: AuthMode::Pin,
            limiter: Limiter::default(),
        }
    }

    pub fn with_mode(mut self, mode: AuthMode) -> Self {
        self.mode = mode;
        self
    }

    /// Open mode: identity is the name the device sends; no session needed.
    pub fn is_open(&self) -> bool {
        self.mode == AuthMode::Open
    }

    fn secure(&self) -> bool {
        self.public_origin
            .as_deref()
            .is_some_and(|o| o.starts_with("https://"))
    }

    fn mac(&self, data: &[u8]) -> Vec<u8> {
        let mut m = HmacSha256::new_from_slice(&self.key).expect("hmac accepts any key length");
        m.update(data);
        m.finalize().into_bytes().to_vec()
    }

    /// Deterministic keyed hash, used to look up join codes without storing them.
    pub fn keyed_hash(&self, purpose: &str, value: &str) -> String {
        hex::encode(self.mac(format!("{purpose}\0{value}").as_bytes()))
    }

    pub fn sign(&self, c: &Claims) -> String {
        let payload = B64.encode(serde_json::to_vec(c).expect("claims serialize"));
        let sig = B64.encode(self.mac(payload.as_bytes()));
        format!("v1.{payload}.{sig}")
    }

    pub fn verify(&self, token: &str) -> Option<Claims> {
        let rest = token.strip_prefix("v1.")?;
        let (payload, sig) = rest.split_once('.')?;
        let sig = B64.decode(sig).ok()?;
        let mut m = HmacSha256::new_from_slice(&self.key).ok()?;
        m.update(payload.as_bytes());
        m.verify_slice(&sig).ok()?;
        serde_json::from_slice(&B64.decode(payload).ok()?).ok()
    }

    pub fn cookie(&self, c: &Claims) -> HeaderValue {
        let max_age = (c.exp - c.iat).max(0);
        let secure = if self.secure() { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}",
            self.sign(c)
        ))
        .expect("cookie is ascii")
    }

    pub fn clear_cookie(&self) -> HeaderValue {
        let secure = if self.secure() { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}"
        ))
        .expect("cookie is ascii")
    }
}

/// Session key from `RFOPS_SESSION_KEY`, else one generated once and kept in the database (fine
/// for a single server; a standby needs the shared env value).
pub async fn load_key(pool: &SqlitePool) -> Result<Vec<u8>, sqlx::Error> {
    if let Ok(k) = std::env::var("RFOPS_SESSION_KEY") {
        if k.len() >= 32 {
            return Ok(k.into_bytes());
        }
        tracing::warn!("RFOPS_SESSION_KEY is shorter than 32 characters; ignoring it");
    }
    if let Some(k) = crate::midi::setting(pool, "session.key").await? {
        return Ok(k.into_bytes());
    }
    let k = random_hex(32);
    crate::midi::set_setting(pool, "session.key", Some(&k)).await?;
    tracing::warn!("no RFOPS_SESSION_KEY set; generated one in the database (set the env var in production so the standby accepts sessions)");
    Ok(k.into_bytes())
}

pub fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

pub fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[serde(rename = "u")]
    User,
    #[serde(rename = "s")]
    Screen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sid: String,
    pub k: Kind,
    pub id: i64,
    /// When the person or screen signed in, in milliseconds (checked against
    /// `sessions_valid_after`, so a PIN change revokes sessions from the same second).
    pub ait: i64,
    /// When this cookie was issued.
    pub iat: i64,
    pub exp: i64,
}

impl Claims {
    pub fn new(k: Kind, id: i64) -> Self {
        let now = unix_now();
        Self {
            sid: random_hex(16),
            k,
            id,
            ait: chrono::Utc::now().timestamp_millis(),
            iat: now,
            exp: now + ttl(k),
        }
    }

    fn renewed(&self) -> Self {
        let now = unix_now();
        Self {
            iat: now,
            exp: now + ttl(self.k),
            ..self.clone()
        }
    }
}

fn ttl(k: Kind) -> i64 {
    match k {
        Kind::User => USER_TTL,
        Kind::Screen => SCREEN_TTL,
    }
}

/// The signed-in person or screen, placed in request extensions by [`require_session`].
#[derive(Debug, Clone, Serialize)]
pub struct Principal {
    pub kind: Kind,
    pub id: i64,
    pub name: String,
    pub role: String,
    pub has_pin: bool,
    #[serde(skip)]
    pub sid: String,
}

/// Endpoints reachable without a session.
pub fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/api/health"
            | "/api/auth/status"
            | "/api/auth/login"
            | "/api/auth/join"
            // Take over checks the Lead's PIN against the downloaded backup itself.
            | "/api/standby/takeover"
            | "/api/standby/status"
            // The on-site agent authenticates with its own key.
            | "/api/midi"
            | "/api/midi/heartbeat"
    )
}

fn cookie_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

/// When the running show entered show mode (unix seconds), or `None` if no show is running.
async fn show_live_since(pool: &SqlitePool) -> Option<i64> {
    let live: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM shows WHERE mode = 'show')")
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    if !live {
        return None;
    }
    let since = crate::midi::setting(pool, "show_mode.since")
        .await
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok());
    // A show already live before this rule existed counts from now.
    Some(since.unwrap_or_else(unix_now))
}

/// Absolute limit from sign-in, even while a show keeps sessions alive.
pub const USER_MAX_AGE: i64 = 7 * 24 * 3600;
pub const SCREEN_MAX_AGE: i64 = 30 * 24 * 3600;

/// Is an expired cookie still accepted? Only while a show is running, only if the cookie was
/// still valid when show mode started, and never beyond the absolute age limit.
async fn kept_alive_by_show(pool: &SqlitePool, c: &Claims) -> bool {
    let max_age = match c.k {
        Kind::User => USER_MAX_AGE,
        Kind::Screen => SCREEN_MAX_AGE,
    };
    if unix_now() - c.ait / 1000 > max_age {
        return false;
    }
    show_live_since(pool).await.is_some_and(|since| c.exp >= since)
}

/// Check a session cookie. Returns the principal and, when due, a renewed cookie.
pub async fn authenticate(
    s: &AppState,
    headers: &HeaderMap,
) -> Result<(Principal, Option<HeaderValue>), ApiError> {
    let unauth = || ApiError::Unauthorized("sign in required".into());
    let token = cookie_value(headers).ok_or_else(unauth)?;
    let claims = s.auth.verify(&token).ok_or_else(unauth)?;
    let now = unix_now();
    // Sessions don't expire mid-show (screens and phones must not drop during a show).
    if claims.exp < now && !kept_alive_by_show(&s.pool, &claims).await {
        return Err(ApiError::Unauthorized(
            "session expired; sign in again".into(),
        ));
    }
    let revoked: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM session_revocations WHERE sid = ?")
            .bind(&claims.sid)
            .fetch_optional(&s.pool)
            .await?;
    if revoked.is_some() {
        return Err(ApiError::Unauthorized("signed out".into()));
    }
    let principal = match claims.k {
        Kind::User => {
            let row: Option<(String, String, bool, Option<String>)> = sqlx::query_as(
                "SELECT name, role, pin_hash IS NOT NULL, sessions_valid_after FROM users WHERE id = ?",
            )
            .bind(claims.id)
            .fetch_optional(&s.pool)
            .await?;
            let (name, role, has_pin, valid_after) = row.ok_or_else(unauth)?;
            let cutoff = valid_after
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(&t).ok())
                .map(|t| t.timestamp_millis());
            if cutoff.is_some_and(|c| claims.ait <= c) {
                return Err(ApiError::Unauthorized("signed out by the Lead".into()));
            }
            Principal {
                kind: Kind::User,
                id: claims.id,
                name,
                role,
                has_pin,
                sid: claims.sid.clone(),
            }
        }
        Kind::Screen => {
            let row: Option<(String, String, bool)> =
                sqlx::query_as("SELECT name, role, revoked <> 0 FROM screens WHERE id = ?")
                    .bind(claims.id)
                    .fetch_optional(&s.pool)
                    .await?;
            let (name, role, revoked) = row.ok_or_else(unauth)?;
            if revoked {
                return Err(ApiError::Unauthorized(
                    "this screen was removed; enrol it again".into(),
                ));
            }
            let _ = sqlx::query("UPDATE screens SET last_seen_at = ? WHERE id = ?")
                .bind(crate::db::now())
                .bind(claims.id)
                .execute(&s.pool)
                .await;
            Principal {
                kind: Kind::Screen,
                id: claims.id,
                name,
                role,
                has_pin: true,
                sid: claims.sid.clone(),
            }
        }
    };
    let renew = (now - claims.iat > REISSUE_AFTER || claims.exp < now)
        .then(|| s.auth.cookie(&claims.renewed()));
    Ok((principal, renew))
}

fn is_ws_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

/// Reject cross-site state changes and WebSocket upgrades. Requests without an Origin header
/// (the agent, curl) are allowed; browsers always send one on these requests.
fn origin_ok(cfg: &AuthConfig, method: &Method, headers: &HeaderMap) -> bool {
    let changes = !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    if !changes && !is_ws_upgrade(headers) {
        return true;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    match &cfg.public_origin {
        Some(expected) => origin == expected,
        None => {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            origin == format!("http://{host}") || origin == format!("https://{host}")
        }
    }
}

/// Same-host origin (`https://<Host>`), used for the takeover page, which is opened on the
/// standby's own onrender.com address rather than the public one.
fn same_host_origin(headers: &HeaderMap) -> bool {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|o| o == format!("https://{host}") || o == format!("http://{host}"))
}

/// Middleware: Origin check, then a session for every non-public `/api/` route.
pub async fn require_session(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    let takeover = req.uri().path() == "/api/standby/takeover";
    let origin_fine = if takeover {
        same_host_origin(req.headers()) || origin_ok(&s.auth, req.method(), req.headers())
    } else {
        origin_ok(&s.auth, req.method(), req.headers())
    };
    if !origin_fine {
        return ApiError::Forbidden("request from another site refused".into()).into_response();
    }
    let path = req.uri().path();
    // A fenced service (the standby, or an old active after a takeover) serves no data at all:
    // its disk may be empty, from a drill, or stale. Only health, the standby endpoints and the
    // app shell (for the takeover page) answer.
    if s.is_fenced()
        && path.starts_with("/api/")
        && path != "/api/health"
        && !path.starts_with("/api/standby/")
    {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "error": "This RF Ops server is not the active one (standby, or taken over). Open the main RF Ops address.",
                "fenced": true,
            })),
        )
            .into_response();
    }
    if s.auth.is_open() || !path.starts_with("/api/") || is_public(path) {
        return next.run(req).await;
    }
    match authenticate(&s, req.headers()).await {
        Ok((p, renew)) => {
            req.extensions_mut().insert(p);
            let mut resp = next.run(req).await;
            if let Some(c) = renew {
                resp.headers_mut().append(header::SET_COOKIE, c);
            }
            resp
        }
        Err(e) => e.into_response(),
    }
}

/// Middleware: security headers on every response.
pub async fn security_headers(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    let set = |h: &mut HeaderMap, k: &'static str, v: &'static str| {
        h.insert(k, HeaderValue::from_static(v));
    };
    set(
        h,
        "content-security-policy",
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'",
    );
    set(h, "x-content-type-options", "nosniff");
    set(h, "x-frame-options", "DENY");
    set(h, "referrer-policy", "same-origin");
    set(h, "permissions-policy", "camera=(self), microphone=(), geolocation=()");
    if s.auth.secure() {
        set(
            h,
            "strict-transport-security",
            "max-age=31536000; includeSubDomains",
        );
    }
    resp
}

pub fn client_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CLIENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.len() <= 100)
        .map(str::to_owned)
}

/// Best-effort client IP behind Cloudflare and Render.
pub fn client_ip(headers: &HeaderMap) -> Option<String> {
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok());
    h("cf-connecting-ip")
        .or_else(|| h("x-forwarded-for").and_then(|v| v.split(',').next()))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------- rate limiting

/// One limit: at most `max` failures within `window`, then locked for `lock`.
#[derive(Debug, Clone)]
pub struct Rule {
    pub key: String,
    pub max: usize,
    pub window: Duration,
    pub lock: Duration,
}

impl Rule {
    pub fn new(key: impl Into<String>, max: usize, window_s: u64, lock_s: u64) -> Self {
        Self {
            key: key.into(),
            max,
            window: Duration::from_secs(window_s),
            lock: Duration::from_secs(lock_s),
        }
    }
}

#[derive(Default)]
pub struct Limiter {
    inner: Mutex<LimState>,
}

#[derive(Default)]
struct LimState {
    fails: HashMap<String, VecDeque<Instant>>,
    locked: HashMap<String, Instant>,
}

impl Limiter {
    /// Seconds until the first locked rule unlocks, if any is locked.
    pub fn blocked(&self, rules: &[Rule]) -> Option<u64> {
        let mut st = self.inner.lock().expect("limiter poisoned");
        let now = Instant::now();
        st.locked.retain(|_, until| *until > now);
        rules
            .iter()
            .filter_map(|r| st.locked.get(&r.key))
            .map(|until| until.duration_since(now).as_secs().max(1))
            .max()
    }

    /// Record a failure against every rule; returns the keys that just became locked.
    pub fn fail(&self, rules: &[Rule]) -> Vec<String> {
        let mut st = self.inner.lock().expect("limiter poisoned");
        let now = Instant::now();
        let mut newly = Vec::new();
        for r in rules {
            let q = st.fails.entry(r.key.clone()).or_default();
            q.push_back(now);
            while q.front().is_some_and(|t| now.duration_since(*t) > r.window) {
                q.pop_front();
            }
            if q.len() >= r.max {
                q.clear();
                st.locked.insert(r.key.clone(), now + r.lock);
                newly.push(r.key.clone());
            }
        }
        newly
    }

    pub fn clear(&self, key: &str) {
        let mut st = self.inner.lock().expect("limiter poisoned");
        st.fails.remove(key);
        st.locked.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AuthConfig {
        AuthConfig::new(b"0123456789abcdef0123456789abcdef".to_vec(), None)
    }

    #[test]
    fn sign_verify_and_tamper() {
        let c = cfg();
        let claims = Claims::new(Kind::User, 7);
        let t = c.sign(&claims);
        let back = c.verify(&t).expect("valid");
        assert_eq!(back.id, 7);
        assert_eq!(back.sid, claims.sid);
        // Change one character of the payload.
        let mut bad = t.clone().into_bytes();
        bad[5] = if bad[5] == b'A' { b'B' } else { b'A' };
        assert!(c.verify(&String::from_utf8(bad).unwrap()).is_none());
        let other = AuthConfig::new(b"another-key-another-key-another-k".to_vec(), None);
        assert!(other.verify(&t).is_none(), "other key rejects");
    }

    #[test]
    fn limiter_locks_after_max_and_clears() {
        let l = Limiter::default();
        let r = [Rule::new("a", 3, 300, 300)];
        assert!(l.fail(&r).is_empty());
        assert!(l.fail(&r).is_empty());
        assert!(l.blocked(&r).is_none());
        assert_eq!(l.fail(&r), vec!["a".to_string()]);
        assert!(l.blocked(&r).is_some());
        assert!(
            l.blocked(&[Rule::new("b", 3, 300, 300)]).is_none(),
            "other keys unaffected"
        );
        l.clear("a");
        assert!(l.blocked(&r).is_none());
    }

    #[test]
    fn origin_rules() {
        let c = AuthConfig::new(b"k".repeat(32), Some("https://rfops.example.com/".into()));
        let mut h = HeaderMap::new();
        assert!(origin_ok(&c, &Method::POST, &h), "no origin (agent)");
        h.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!origin_ok(&c, &Method::POST, &h));
        assert!(origin_ok(&c, &Method::GET, &h), "plain GET");
        h.insert(header::UPGRADE, "websocket".parse().unwrap());
        assert!(!origin_ok(&c, &Method::GET, &h), "cross-site WS");
        h.insert(header::ORIGIN, "https://rfops.example.com".parse().unwrap());
        assert!(origin_ok(&c, &Method::GET, &h));
    }
}
