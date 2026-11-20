//! Reliability foundations: verified/pinned backups, startup recovery, heartbeat, presence,
//! health.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use rfops_server::{app, backup, db, seed, AppState};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn tmpdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("rfops-rel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn durable_pragmas_are_set() {
    let d = tmpdir("pragmas");
    let pool = db::connect(&format!("sqlite://{}", d.join("a.db").display()))
        .await
        .unwrap();
    let sync: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sync, 2, "synchronous=FULL");
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    let busy: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(busy >= 5000);
    db::quick_check(&pool).await.unwrap();
}

#[tokio::test]
async fn interrupted_backups_never_listed_and_pinned_survive_rotation() {
    let d = tmpdir("rotate");
    let pool = db::connect(&format!("sqlite://{}", d.join("a.db").display()))
        .await
        .unwrap();
    let bdir = d.join("backups");
    std::fs::create_dir_all(&bdir).unwrap();
    // A leftover from an interrupted backup, plus an old pinned show-start and many autos.
    std::fs::write(bdir.join(".rfops-20260101-000000.000.tmp"), b"partial").unwrap();
    std::fs::write(
        bdir.join("rfops-20260101-000000.000-show-start-pin.db"),
        b"x",
    )
    .unwrap();
    for i in 0..(backup::KEEP_AUTO + 5) {
        std::fs::write(
            bdir.join(format!("rfops-20260102-{i:06}.000-auto.db")),
            b"x",
        )
        .unwrap();
    }
    assert!(backup::list(&bdir)
        .iter()
        .all(|f| !f.name.ends_with(".tmp")));
    let f = backup::run(&pool, &bdir, "auto").await.unwrap();
    assert!(f.verified && !f.pinned);
    let after = backup::list(&bdir);
    assert!(
        after.iter().any(|f| f.name.ends_with("show-start-pin.db")),
        "pinned kept"
    );
    assert_eq!(
        after.iter().filter(|f| !f.pinned).count(),
        backup::KEEP_AUTO
    );
    assert!(
        !bdir.join(".rfops-20260101-000000.000.tmp").exists(),
        "temp cleaned"
    );
    let m = backup::run(&pool, &bdir, "manual").await.unwrap();
    assert!(m.pinned);
}

#[tokio::test]
async fn corrupt_database_is_moved_aside_and_backup_restored() {
    let d = tmpdir("corrupt");
    let url = format!("sqlite://{}", d.join("live.db").display());
    let bdir = d.join("backups");
    {
        let pool = db::connect(&url).await.unwrap();
        seed::seed_demo(&pool).await.unwrap();
        backup::run(&pool, &bdir, "manual").await.unwrap();
        pool.close().await;
    }
    // Clobber page 1 onwards (keep the header so it still "opens") to fail quick_check.
    let path = d.join("live.db");
    let mut bytes = std::fs::read(&path).unwrap();
    for b in bytes.iter_mut().skip(100).take(8192) {
        *b = 0xAB;
    }
    std::fs::write(&path, &bytes).unwrap();
    let _ = std::fs::remove_file(d.join("live.db-wal"));
    let _ = std::fs::remove_file(d.join("live.db-shm"));

    let (pool, note) = db::open_verified(&url, Some(&bdir))
        .await
        .expect("recovers");
    let note = note.expect("recovery note");
    assert!(note.contains("restored"), "{note}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM performers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 11, "data from the backup");
    let aside = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("corrupt-"));
    assert!(aside, "damaged file kept, not deleted");

    // No backup available: refuse with a clear message.
    let d2 = tmpdir("corrupt2");
    let p2 = d2.join("x.db");
    std::fs::write(&p2, &bytes).unwrap();
    let err = db::open_verified(
        &format!("sqlite://{}", p2.display()),
        Some(&d2.join("none")),
    )
    .await
    .map(|_| ())
    .expect_err("refuses");
    assert!(err.contains("no verified backup"), "{err}");
}

async fn start() -> (String, String) {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    seed::seed_demo(&pool).await.unwrap();
    let state = AppState::new(pool).await.unwrap().insecure_header_auth_for_tests().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    (format!("http://{addr}"), format!("ws://{addr}/api/ws"))
}

async fn next_json(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    within: Duration,
) -> Value {
    loop {
        let m = tokio::time::timeout(within, ws.next())
            .await
            .expect("message in time")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

async fn get_json(url: &str, actor: Option<&str>) -> (u16, Value) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let u = url.trim_start_matches("http://");
    let (host, path) = u.split_once('/').unwrap();
    let mut s = tokio::net::TcpStream::connect(host).await.unwrap();
    let extra = actor
        .map(|a| format!("x-rfops-actor: {a}\r\n"))
        .unwrap_or_default();
    s.write_all(
        format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

#[tokio::test]
async fn hello_carries_epoch_and_ticks_arrive_and_presence_tracks_screens() {
    let (base, ws) = start().await;
    let (mut c, _) = connect_async(format!(
        "{ws}?client=wall-1&operator=A2%20Stage%20Left&screen=wall"
    ))
    .await
    .unwrap();
    let hello = next_json(&mut c, Duration::from_secs(2)).await;
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["epoch"], 1);
    assert!(hello["server_time"].is_string());

    let (_, screens) = get_json(&format!("{base}/api/presence"), None).await;
    assert_eq!(screens[0]["client_id"], "wall-1");
    assert_eq!(screens[0]["screen"], "wall");

    // Heartbeat tick within 10 s (+ margin), carrying server time and epoch.
    let tick = next_json(&mut c, Duration::from_secs(12)).await;
    assert_eq!(tick["type"], "tick");
    assert_eq!(tick["epoch"], 1);

    c.send(Message::Close(None)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, screens) = get_json(&format!("{base}/api/presence"), None).await;
    assert!(
        screens.as_array().unwrap().is_empty(),
        "removed on disconnect"
    );
}

#[tokio::test]
async fn health_endpoints() {
    let (base, _) = start().await;
    let (s, v) = get_json(&format!("{base}/api/health"), None).await;
    assert_eq!(s, 200);
    assert_eq!(v["ok"], true);
    assert!(
        v.get("database").is_none(),
        "public health shows status only"
    );
    let (s, d) = get_json(&format!("{base}/api/health/detail"), Some("Admin")).await;
    assert_eq!(s, 200, "{d}");
    assert_eq!(d["database"]["integrity"], "ok");
    assert_eq!(d["epoch"], 1);
    assert!(d["uptime_s"].is_number());
}
