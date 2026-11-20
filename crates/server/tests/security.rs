//! PIN sign-in mode (`RFOPS_AUTH=pin`, step 1b): everything behind a session, PIN and join-code sign-in, lockouts,
//! screens, revocation, Origin checks and security headers. Runs with real auth (no header
//! identity).

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use rfops_server::{app, auth, auth::AuthMode, db, seed, upsert_admin, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

struct T {
    app: Router,
    pool: sqlx::SqlitePool,
    state: AppState,
}

struct Res {
    status: StatusCode,
    body: Value,
    cookies: Vec<String>,
    headers: axum::http::HeaderMap,
}

impl Res {
    /// `rfops_session=...` from Set-Cookie, ready for a Cookie header.
    fn session(&self) -> String {
        self.cookies
            .iter()
            .find(|c| c.starts_with("rfops_session=") && !c.starts_with("rfops_session=;"))
            .map(|c| c.split(';').next().unwrap().to_string())
            .expect("session cookie set")
    }
    fn device(&self) -> String {
        self.cookies
            .iter()
            .find(|c| c.starts_with("rfops_device="))
            .map(|c| c.split(';').next().unwrap().to_string())
            .expect("device cookie set")
    }
}

impl T {
    async fn new() -> Self {
        let pool = db::connect("sqlite::memory:").await.unwrap();
        seed::seed_demo(&pool).await.unwrap();
        upsert_admin(&pool, "Lead", "246810").await.unwrap();
        let state = AppState::new(pool.clone())
            .await
            .unwrap()
            .with_auth_mode(AuthMode::Pin)
            .await;
        T {
            app: app(state.clone()),
            pool,
            state,
        }
    }

    async fn call(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Res {
        let mut b = Request::builder().method(method).uri(uri);
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let req = match body {
            Some(v) => b
                .header("content-type", "application/json")
                .body(Body::from(v.to_string())),
            None => b.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let cookies = headers
            .get_all("set-cookie")
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        Res {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            cookies,
            headers,
        }
    }

    async fn login(&self, name: &str, pin: &str, device: &str) -> Res {
        self.call(
            "POST",
            "/api/auth/login",
            Some(json!({ "name": name, "pin": pin })),
            &[("x-rfops-client", device)],
        )
        .await
    }

    async fn lead(&self) -> String {
        let r = self.login("Lead", "246810", "lead-laptop").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.session()
    }

    async fn user_id(&self, name: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM users WHERE name = ?")
            .bind(name)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

const PROTECTED_GETS: &[&str] = &[
    "/api/snapshot",
    "/api/shows",
    "/api/equipment",
    "/api/users",
    "/api/alerts",
    "/api/events",
    "/api/backups",
    "/api/presence",
    "/api/checkouts",
    "/api/health/detail",
    "/api/midi/status",
    "/api/export/equipment.csv",
    "/api/export/performers.csv",
    "/api/auth/screens",
    "/api/shows/1",
    "/api/shows/1/handoffs",
];

#[tokio::test]
async fn everything_needs_a_session_except_public_endpoints() {
    let t = T::new().await;
    for path in PROTECTED_GETS {
        let r = t.call("GET", path, None, &[]).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "GET {path}");
    }
    // Legacy header identity is ignored by the real server.
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("x-rfops-actor", "Admin")],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // WebSocket upgrade is refused before upgrading.
    let r = t
        .call(
            "GET",
            "/api/ws",
            None,
            &[
                ("connection", "upgrade"),
                ("upgrade", "websocket"),
                ("sec-websocket-version", "13"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // Public: status only.
    let r = t.call("GET", "/api/health", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.get("database").is_none());
    let r = t.call("GET", "/api/auth/status", None, &[]).await;
    assert_eq!(r.body["admin_exists"], true);
    assert!(r.body["signed_in"].is_null());
}

#[tokio::test]
async fn pin_sign_in_cookie_and_sign_out() {
    let t = T::new().await;
    // Seeded operators have no PIN: they can't sign in by name alone.
    let r = t.login("A2 Stage Left", "", "phone-1").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert!(r.body["error"].as_str().unwrap().contains("PIN"));

    let lead = t.lead().await;
    let r = t
        .call("GET", "/api/snapshot", None, &[("cookie", &lead)])
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let set = t.login("Lead", "246810", "x").await;
    let c = set.cookies.iter().find(|c| c.starts_with("rfops_session=")).unwrap();
    assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Max-Age="));

    // Lead sets the A2's PIN; the A2 signs in and can write.
    let left = t.user_id("A2 Stage Left").await;
    let r = t
        .call(
            "PUT",
            &format!("/api/users/{left}/pin"),
            Some(json!({ "pin": "4821" })),
            &[("cookie", &lead)],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let a2 = t.login("a2 stage left", "4821", "phone-1").await;
    assert_eq!(a2.status, StatusCode::OK, "names are case-insensitive");
    let a2c = a2.session();
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("cookie", &a2c)],
        )
        .await;
    assert!(r.status.is_success(), "{}", r.body);
    // Attribution comes from the session.
    let actor: String =
        sqlx::query_scalar("SELECT actor FROM event_log ORDER BY seq DESC LIMIT 1")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(actor, "A2 Stage Left");

    // Health detail is Lead-only.
    let r = t
        .call("GET", "/api/health/detail", None, &[("cookie", &a2c)])
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Sign out revokes that cookie.
    let r = t
        .call("POST", "/api/auth/logout", None, &[("cookie", &a2c)])
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = t
        .call("GET", "/api/snapshot", None, &[("cookie", &a2c)])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // A PIN change signs that operator out everywhere, immediately.
    let a2c = t.login("A2 Stage Left", "4821", "phone-1").await.session();
    t.call(
        "PUT",
        &format!("/api/users/{left}/pin"),
        Some(json!({ "pin": "5555" })),
        &[("cookie", &lead)],
    )
    .await;
    let r = t
        .call("GET", "/api/snapshot", None, &[("cookie", &a2c)])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // Non-helpers can't have their PIN removed.
    let r = t
        .call(
            "PUT",
            &format!("/api/users/{left}/pin"),
            Some(json!({ "pin": null })),
            &[("cookie", &lead)],
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // A tampered cookie is rejected.
    let mut bad = lead.clone();
    bad.push('x');
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &bad)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn lockout_is_per_name_and_device_and_trusted_devices_survive() {
    let t = T::new().await;
    let trusted = t.login("Lead", "246810", "lead-phone").await.device();
    for _ in 0..5 {
        let r = t.login("Lead", "0000", "attacker-1").await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let r = t.login("Lead", "246810", "attacker-1").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "locked on that device");
    let r = t.login("Lead", "246810", "lead-laptop").await;
    assert_eq!(r.status, StatusCode::OK, "other device unaffected");

    // Rotating device ids: 20 failures across devices pause the name for unknown devices…
    for i in 0..20 {
        t.login("Lead", "0000", &format!("rot-{i}")).await;
    }
    let r = t.login("Lead", "246810", "brand-new").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    // …but a device that signed in before still gets in.
    let r = t
        .call(
            "POST",
            "/api/auth/login",
            Some(json!({ "name": "Lead", "pin": "246810" })),
            &[("x-rfops-client", "lead-phone"), ("cookie", &trusted)],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // The Lead is warned.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alerts WHERE dedupe_key = 'auth:name:lead' AND state <> 'resolved'",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn helper_join_code_three_devices_two_hours() {
    let t = T::new().await;
    let lead = t.lead().await;
    let r = t
        .call(
            "POST",
            "/api/users",
            Some(json!({ "name": "Jo", "role": "helper" })),
            &[("cookie", &lead)],
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let jo = r.body["id"].as_i64().unwrap();
    // Join codes are for helpers only.
    let a2 = t.user_id("A2 Stage Left").await;
    let r = t
        .call("POST", "/api/auth/join-codes", Some(json!({ "user_id": a2 })), &[("cookie", &lead)])
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = t
        .call("POST", "/api/auth/join-codes", Some(json!({ "user_id": jo })), &[("cookie", &lead)])
        .await;
    let code = r.body["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);
    assert_eq!(r.body["uses"], 3);
    let stored: String = sqlx::query_scalar("SELECT code_hash FROM join_codes")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_ne!(stored, code, "code is not stored in clear");

    for dev in ["safari", "home-screen", "ipad"] {
        let r = t
            .call(
                "POST",
                "/api/auth/join",
                Some(json!({ "code": format!("{} {}", &code[..3], &code[3..]) })),
                &[("x-rfops-client", dev)],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{dev}: {}", r.body);
        assert_eq!(r.body["user"]["name"], "Jo");
    }
    let r = t
        .call("POST", "/api/auth/join", Some(json!({ "code": code })), &[("x-rfops-client", "4th")])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "fourth device refused");

    // A fresh code expires after 2 h.
    let r = t
        .call("POST", "/api/auth/join-codes", Some(json!({ "user_id": jo })), &[("cookie", &lead)])
        .await;
    let code = r.body["code"].as_str().unwrap().to_string();
    sqlx::query("UPDATE join_codes SET expires_at = ?")
        .bind(auth::unix_now() - 1)
        .execute(&t.pool)
        .await
        .unwrap();
    let r = t
        .call("POST", "/api/auth/join", Some(json!({ "code": code })), &[("x-rfops-client", "late")])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Helpers can't sign in by name.
    let r = t.login("Jo", "", "x").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Re-issue all cancels outstanding codes.
    let r = t
        .call("POST", "/api/auth/join-codes", Some(json!({ "user_id": jo })), &[("cookie", &lead)])
        .await;
    let code = r.body["code"].as_str().unwrap().to_string();
    t.call("DELETE", "/api/auth/join-codes", None, &[("cookie", &lead)]).await;
    let r = t
        .call("POST", "/api/auth/join", Some(json!({ "code": code })), &[("x-rfops-client", "y")])
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn join_guessing_is_capped_globally() {
    let t = T::new().await;
    for i in 0..20 {
        t.call(
            "POST",
            "/api/auth/join",
            Some(json!({ "code": "000000" })),
            &[("x-rfops-client", &format!("g{i}"))],
        )
        .await;
    }
    let r = t
        .call("POST", "/api/auth/join", Some(json!({ "code": "123456" })), &[("x-rfops-client", "new")])
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn screens_enrol_attribute_and_revoke() {
    let t = T::new().await;
    let lead = t.lead().await;
    let r = t
        .call(
            "POST",
            "/api/auth/screens",
            Some(json!({ "name": "Wall SL", "role": "a2" })),
            &[("cookie", &lead)],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let wall = r.session();
    let c = r.cookies.iter().find(|c| c.starts_with("rfops_session=")).unwrap();
    assert!(c.contains(&format!("Max-Age={}", auth::SCREEN_TTL)));
    // The Lead's session on that browser was replaced.
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &lead)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // "Wall SL (Sam)" when someone tapped their name chip; unknown names are ignored.
    let alert = json!({ "severity": "info", "title": "x" });
    t.call(
        "POST",
        "/api/alerts",
        Some(alert.clone()),
        &[("cookie", &wall), ("x-rfops-on-behalf", "A2 Stage Left")],
    )
    .await;
    t.call(
        "POST",
        "/api/alerts",
        Some(alert.clone()),
        &[("cookie", &wall), ("x-rfops-on-behalf", "Mallory")],
    )
    .await;
    let actors: Vec<String> =
        sqlx::query_scalar("SELECT actor FROM event_log ORDER BY seq DESC LIMIT 2")
            .fetch_all(&t.pool)
            .await
            .unwrap();
    assert_eq!(actors, vec!["Wall SL", "Wall SL (A2 Stage Left)"]);

    // Screens can't do Lead things.
    let r = t
        .call("POST", "/api/auth/screens", Some(json!({ "name": "X" })), &[("cookie", &wall)])
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let lead = t.lead().await;
    let id: i64 = sqlx::query_scalar("SELECT id FROM screens WHERE name = 'Wall SL'")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    t.call("DELETE", &format!("/api/auth/screens/{id}"), None, &[("cookie", &lead)])
        .await;
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &wall)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn show_mode_keeps_sessions_alive_within_limits() {
    let t = T::new().await;
    let lead_id = t.user_id("Lead").await;
    let now = auth::unix_now();
    let mk = |iat: i64, exp: i64, ait_s: i64| {
        let mut c = auth::Claims::new(auth::Kind::User, lead_id);
        c.iat = iat;
        c.exp = exp;
        c.ait = ait_s * 1000;
        format!("rfops_session={}", t.state.auth.sign(&c))
    };
    // Valid when the show started, expired since: kept alive.
    let kept = mk(now - 20 * 3600, now - 60, now - 20 * 3600);
    // Already expired before the show started: not revived.
    let stale = mk(now - 40 * 3600, now - 20 * 3600, now - 40 * 3600);
    // Older than the 7-day absolute limit.
    let ancient = mk(now - 3600, now - 60, now - 8 * 24 * 3600);

    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &kept)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "no show running");

    sqlx::query("UPDATE shows SET mode = 'show' WHERE id = (SELECT MIN(id) FROM shows)")
        .execute(&t.pool)
        .await
        .unwrap();
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('show_mode.since', ?)")
        .bind((now - 3 * 3600).to_string())
        .execute(&t.pool)
        .await
        .unwrap();
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &kept)]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.cookies.iter().any(|c| c.starts_with("rfops_session=")),
        "renewed"
    );
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &stale)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &ancient)]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn origin_check_and_security_headers() {
    let t = T::new().await;
    let lead = t.lead().await;
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("cookie", &lead), ("origin", "https://evil.example"), ("host", "rfops.test")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("cookie", &lead), ("origin", "http://rfops.test"), ("host", "rfops.test")],
        )
        .await;
    assert!(r.status.is_success(), "{}", r.body);
    let r = t.call("GET", "/api/health", None, &[]).await;
    let csp = r.headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"));
    assert_eq!(r.headers["x-frame-options"], "DENY");
    assert_eq!(r.headers["x-content-type-options"], "nosniff");
}

#[tokio::test]
async fn viewers_and_pinless_operators_cannot_write() {
    let t = T::new().await;
    let lead = t.lead().await;
    let r = t
        .call(
            "POST",
            "/api/users",
            Some(json!({ "name": "Watcher", "role": "viewer", "pin": "1111" })),
            &[("cookie", &lead)],
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let v = t.login("Watcher", "1111", "v").await.session();
    let r = t.call("GET", "/api/snapshot", None, &[("cookie", &v)]).await;
    assert_eq!(r.status, StatusCode::OK, "viewers can read");
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("cookie", &v)],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn open_mode_needs_only_a_name() {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    seed::seed_demo(&pool).await.unwrap();
    let state = AppState::new(pool.clone()).await.unwrap().open_mode().await;
    let t = T { app: app(state.clone()), pool, state };
    let r = t.call("GET", "/api/snapshot", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK, "reads need nothing");
    let r = t.call("GET", "/api/auth/status", None, &[]).await;
    assert_eq!(r.body["mode"], "open");
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("x-rfops-actor", "Somebody New")],
        )
        .await;
    assert!(r.status.is_success(), "any name may act: {}", r.body);
    let actor: String = sqlx::query_scalar("SELECT actor FROM event_log ORDER BY seq DESC LIMIT 1")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    assert_eq!(actor, "Somebody New");
    let r = t
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &[("x-rfops-actor", "Viewer")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "a configured viewer stays read-only");
}
