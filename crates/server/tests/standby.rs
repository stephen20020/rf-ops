//! Standby fencing and R2 backups against an in-process S3 mock (reliability req 26).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use rfops_server::{
    app, backup, db,
    lease::{Lease, LeaseHandle, KEY},
    r2::{Cond, R2},
    seed, upsert_admin, AppState,
};
use serde_json::json;
use tokio::net::TcpListener;

type Store = Arc<Mutex<HashMap<String, (Vec<u8>, u64)>>>;

/// Minimal S3: PUT with If-Match / If-None-Match, GET, ListObjectsV2. ETag = a version counter.
async fn mock_r2() -> (R2, Store) {
    let store: Store = Arc::default();
    async fn put(
        State(st): State<Store>,
        Path((_b, key)): Path<(String, String)>,
        h: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        let mut m = st.lock().unwrap();
        let cur = m.get(&key).map(|(_, v)| format!("\"{v}\""));
        if let Some(want) = h.get("if-match").and_then(|v| v.to_str().ok()) {
            if cur.as_deref() != Some(want) {
                return (StatusCode::PRECONDITION_FAILED, HeaderMap::new());
            }
        }
        if h.get("if-none-match").is_some() && cur.is_some() {
            return (StatusCode::PRECONDITION_FAILED, HeaderMap::new());
        }
        let v = m.get(&key).map(|(_, v)| v + 1).unwrap_or(1);
        m.insert(key, (body.to_vec(), v));
        let mut out = HeaderMap::new();
        out.insert("etag", format!("\"{v}\"").parse().unwrap());
        (StatusCode::OK, out)
    }
    async fn get_obj(
        State(st): State<Store>,
        Path((_b, key)): Path<(String, String)>,
    ) -> impl IntoResponse {
        let m = st.lock().unwrap();
        match m.get(&key) {
            Some((b, v)) => {
                let mut h = HeaderMap::new();
                h.insert("etag", format!("\"{v}\"").parse().unwrap());
                (StatusCode::OK, h, b.clone()).into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
    async fn list(
        State(st): State<Store>,
        Query(q): Query<HashMap<String, String>>,
    ) -> impl IntoResponse {
        let prefix = q.get("prefix").cloned().unwrap_or_default();
        let m = st.lock().unwrap();
        let items: String = m
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, (b, _))| format!("<Contents><Key>{k}</Key><Size>{}</Size></Contents>", b.len()))
            .collect();
        format!("<ListBucketResult><IsTruncated>false</IsTruncated>{items}</ListBucketResult>")
    }
    let app = Router::new()
        .route("/{bucket}", get(list))
        .route("/{bucket}/{*key}", get(get_obj).put(put))
        .with_state(store.clone());
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (
        R2::new(&format!("http://{addr}"), "rfops", "test-key", "test-secret"),
        store,
    )
}

fn lease_in(store: &Store) -> Lease {
    serde_json::from_slice(&store.lock().unwrap()[KEY].0).unwrap()
}

#[tokio::test]
async fn active_creates_lease_standby_starts_fenced() {
    let (r2, store) = mock_r2().await;
    let a = LeaseHandle::start(r2.clone(), "active").await;
    assert!(!a.is_fenced());
    assert_eq!(lease_in(&store).generation, 1);
    let s = LeaseHandle::start(r2.clone(), "standby").await;
    assert!(s.is_fenced(), "standby always starts fenced");
    // A restarted active resumes its own lease.
    let a2 = LeaseHandle::start(r2, "active").await;
    assert!(!a2.is_fenced());
}

#[tokio::test]
async fn takeover_fences_the_old_active_on_its_next_renewal() {
    let (r2, store) = mock_r2().await;
    let a = LeaseHandle::start(r2.clone(), "active").await;
    let s = LeaseHandle::start(r2.clone(), "standby").await;
    let g = s.take_over().await.unwrap();
    assert_eq!(g, 2);
    assert!(s.is_fenced(), "stays fenced until it has restarted into the restored DB");
    assert_eq!(lease_in(&store).holder, "standby");
    a.renew().await;
    assert!(a.is_fenced(), "412 with another holder fences");
    // The old active restarting while the standby's lease is fresh starts fenced.
    let a2 = LeaseHandle::start(r2.clone(), "active").await;
    assert!(a2.is_fenced());
    // The standby restarting resumes its lease.
    let s2 = LeaseHandle::start(r2, "standby").await;
    assert!(!s2.is_fenced());
    assert_eq!(s2.generation(), 2);
}

#[tokio::test]
async fn nobody_takes_a_lease_it_does_not_hold_even_expired() {
    let (r2, store) = mock_r2().await;
    let old = Lease {
        generation: 4,
        holder: "standby".into(),
        renewed_at: chrono::Utc::now().timestamp() - 3600,
    };
    r2.put(KEY, serde_json::to_vec(&old).unwrap(), Cond::None)
        .await
        .unwrap();
    // Fail back: the active must not come up on its stale disk just because the lease expired.
    let a = LeaseHandle::start(r2.clone(), "active").await;
    assert!(a.is_fenced());
    assert_eq!(lease_in(&store).generation, 4, "untouched");
    // Only Take over gives it the lease (after restoring the newest backup).
    assert_eq!(a.take_over().await.unwrap(), 5);
}

#[tokio::test]
async fn r2_down_at_start_serves_only_the_last_holder() {
    let dead = R2::new("http://127.0.0.1:9", "rfops", "k", "s");
    let a = LeaseHandle::start_with(dead.clone(), "active", true).await;
    assert!(!a.is_fenced(), "ordinary restart during an R2 outage keeps serving");
    let b = LeaseHandle::start_with(dead, "active", false).await;
    assert!(b.is_fenced());
}

#[tokio::test]
async fn concurrent_takeovers_have_one_winner() {
    let (r2, _) = mock_r2().await;
    let _a = LeaseHandle::start(r2.clone(), "active").await;
    let x = LeaseHandle::start(r2.clone(), "standby").await;
    let y = LeaseHandle::start(r2, "standby-2").await;
    let (rx, ry) = tokio::join!(x.take_over(), y.take_over());
    assert_eq!(
        [rx.is_ok(), ry.is_ok()].iter().filter(|ok| **ok).count(),
        1,
        "{rx:?} {ry:?}"
    );
}

async fn file_state(dir: &std::path::Path, seed_it: bool) -> (AppState, sqlx::SqlitePool) {
    let url = format!("sqlite://{}", dir.join("live.db").display());
    let pool = db::connect(&url).await.unwrap();
    if seed_it {
        seed::seed_demo(&pool).await.unwrap();
        upsert_admin(&pool, "Lead", "246810").await.unwrap();
    }
    (AppState::new(pool.clone()).await.unwrap(), pool)
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("rfops-standby-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn backups_go_to_r2_under_the_generation_and_takeover_checks_the_pin() {
    let (r2, store) = mock_r2().await;

    // The active: seeded, holds the lease, uploads a backup.
    let da = tmp("active");
    let (sa, _pa) = file_state(&da, true).await;
    let la = LeaseHandle::start(r2.clone(), "active").await;
    let sa = sa
        .with_offsite(backup::r2_offsite(r2.clone(), Some(la.clone())))
        .with_lease(la.clone())
        .with_backups(
            da.join("backups"),
            std::time::Duration::from_secs(3600),
            std::time::Duration::from_secs(3600),
        );
    let f = sa.backup_now("manual").await.unwrap().unwrap();
    let key = backup::r2_key(1, &f.name);
    assert!(store.lock().unwrap().contains_key(&key), "uploaded as {key}");
    assert!(key.starts_with("pinned/g000001/"));

    // The standby: empty disk, fenced, refuses writes.
    let ds = tmp("standby");
    let (ss, _ps) = file_state(&ds, false).await;
    let ls = LeaseHandle::start(r2.clone(), "standby").await;
    let ss = ss.with_lease(ls.clone());
    let app_s = app(ss.clone());
    let call = |method: &str, uri: &str, body: serde_json::Value| {
        let app_s = app_s.clone();
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap();
        async move {
            use tower::ServiceExt;
            let r = app_s.oneshot(req).await.unwrap();
            let st = r.status();
            let b = http_body_util::BodyExt::collect(r.into_body())
                .await
                .unwrap()
                .to_bytes();
            (st, serde_json::from_slice::<serde_json::Value>(&b).unwrap_or_default())
        }
    };
    let (st, v) = call("POST", "/api/auth/login", json!({ "name": "Lead", "pin": "246810" })).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "fenced refuses writes: {v}");
    let (st, _) = call("GET", "/api/auth/status", json!(null)).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "fenced serves no data either");
    let (st, v) = call("GET", "/api/standby/status", json!(null)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["fenced"], true);
    assert!(v["newest_backup"]["key"].as_str().unwrap().starts_with("pinned/g000001/"));

    // Wrong PIN (as of the backup) is refused, and nothing changes.
    let (st, _) = call("POST", "/api/standby/takeover", json!({ "name": "Lead", "pin": "000000" })).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(lease_in(&store).holder, "active");

    // Right PIN: lease taken (generation 2), restore marker written, still fenced until restart.
    let (st, v) = call("POST", "/api/standby/takeover", json!({ "name": "Lead", "pin": "246810" })).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["generation"], 2);
    assert_eq!(lease_in(&store).holder, "standby");
    let marker = std::path::PathBuf::from(format!("{}.restore-on-boot", ds.join("live.db").display()));
    let staged = std::fs::read_to_string(&marker).unwrap();
    db::quick_check_file(std::path::Path::new(staged.trim()))
        .await
        .unwrap();
    assert!(ls.is_fenced());

    // The old active is fenced at its next renewal and stops uploading.
    la.renew().await;
    assert!(la.is_fenced());
    let r = sa.backup_now("manual").await.unwrap();
    assert!(r.is_ok(), "local backup still written");
    assert_eq!(
        sa.backup_status.lock().unwrap().last_offsite_error.as_deref(),
        Some("fenced: not uploading")
    );
    // And refuses writes.
    let app_a = app(sa.clone());
    use tower::ServiceExt;
    let r = app_a
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"name":"Lead","pin":"246810"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
}
