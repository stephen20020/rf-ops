//! Phase 2: real-time sync, idempotency, roles, song advance, handoff tracking, demo seed.

use std::time::Duration;

use futures_util::StreamExt;
use rfops_server::{app, db, seed, AppState};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

struct Srv {
    base: String,
    ws: String,
    pool: sqlx::SqlitePool,
    http: Client,
}

/// Minimal HTTP client over tokio (avoids pulling in reqwest).
#[derive(Clone)]
struct Client {
    addr: String,
}

impl Client {
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (u16, Value, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(&self.addr).await.unwrap();
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n",
            self.addr,
            body.len()
        );
        if !body.is_empty() {
            req.push_str("Content-Type: application/json\r\n");
        }
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&body);
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, rest) = text.split_once("\r\n\r\n").unwrap();
        let status: u16 = head.split(' ').nth(1).unwrap().parse().unwrap();
        let body = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            dechunk(rest)
        } else {
            rest.to_string()
        };
        let v = serde_json::from_str(&body).unwrap_or(Value::Null);
        (status, v, head.to_string())
    }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&after[..n]);
        rest = &after[n + 2..];
    }
    out
}

impl Srv {
    async fn start() -> Self {
        Self::start_with(db::connect("sqlite::memory:").await.unwrap()).await
    }

    async fn start_with(pool: sqlx::SqlitePool) -> Self {
        let state = AppState::new(pool.clone()).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
        Srv {
            base: format!("http://{addr}"),
            ws: format!("ws://{addr}/api/ws"),
            pool,
            http: Client { addr },
        }
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        let (s, v, _) = self
            .http
            .call("POST", path, Some(body), &[("x-rfops-actor", "tester")])
            .await;
        assert!((200..300).contains(&s), "POST {path} -> {s} {v}");
        v
    }

    async fn get(&self, path: &str) -> Value {
        let (s, v, _) = self.http.call("GET", path, None, &[]).await;
        assert_eq!(s, 200, "GET {path}: {v}");
        v
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_json(ws: &mut Ws) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("timed out waiting for websocket message")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[tokio::test]
async fn two_clients_receive_events_quickly() {
    let srv = Srv::start().await;
    let (mut a, _) = connect_async(&srv.ws).await.unwrap();
    let (mut b, _) = connect_async(&srv.ws).await.unwrap();
    assert_eq!(next_json(&mut a).await["type"], "hello");
    assert_eq!(next_json(&mut b).await["type"], "hello");

    let started = std::time::Instant::now();
    srv.post(
        "/api/alerts",
        json!({ "severity": "critical", "title": "HH3 dropout" }),
    )
    .await;
    for ws in [&mut a, &mut b] {
        let m = next_json(ws).await;
        assert_eq!(m["type"], "event");
        assert_eq!(m["event"]["type"], "alert.opened");
        assert_eq!(m["event"]["payload"]["title"], "HH3 dropout");
    }
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "took {:?}",
        started.elapsed()
    );
    let _ = srv.base.len();
}

#[tokio::test]
async fn reconnect_replays_missed_events() {
    let srv = Srv::start().await;
    srv.post("/api/performers", json!({ "name": "A" })).await;
    let seq = srv.get("/api/snapshot").await["seq"].as_i64().unwrap();
    // Two writes while "disconnected".
    srv.post("/api/performers", json!({ "name": "B" })).await;
    srv.post("/api/performers", json!({ "name": "C" })).await;

    let (mut ws, _) = connect_async(format!("{}?after={seq}", srv.ws))
        .await
        .unwrap();
    assert_eq!(next_json(&mut ws).await["type"], "hello");
    let e1 = next_json(&mut ws).await;
    let e2 = next_json(&mut ws).await;
    assert_eq!(e1["event"]["payload"]["name"], "B");
    assert_eq!(e2["event"]["payload"]["name"], "C");
    assert!(e2["event"]["seq"].as_i64() > e1["event"]["seq"].as_i64());

    // Live events continue after the replay, with no duplicates.
    srv.post("/api/performers", json!({ "name": "D" })).await;
    let e3 = next_json(&mut ws).await;
    assert_eq!(e3["event"]["payload"]["name"], "D");
}

#[tokio::test]
async fn idempotent_retry_does_not_double_apply() {
    let srv = Srv::start().await;
    let h = [
        ("x-rfops-actor", "tester"),
        ("x-rfops-idempotency-key", "abc-123"),
    ];
    let body = json!({ "severity": "warning", "title": "IEM 4 crackle" });
    let (s1, v1, h1) = srv
        .http
        .call("POST", "/api/alerts", Some(body.clone()), &h)
        .await;
    let (s2, v2, h2) = srv.http.call("POST", "/api/alerts", Some(body), &h).await;
    assert_eq!(s1, 201);
    assert_eq!(s2, 201);
    assert_eq!(v1["id"], v2["id"]);
    assert!(!h1.contains("x-rfops-replayed"));
    assert!(h2.contains("x-rfops-replayed"));
    assert_eq!(srv.get("/api/alerts").await.as_array().unwrap().len(), 1);

    // Acknowledge retried: second call must not fail with "already acknowledged".
    let id = v1["id"].as_i64().unwrap();
    let h = [
        ("x-rfops-actor", "tester"),
        ("x-rfops-idempotency-key", "ack-1"),
    ];
    let (s1, _, _) = srv
        .http
        .call("POST", &format!("/api/alerts/{id}/ack"), None, &h)
        .await;
    let (s2, _, _) = srv
        .http
        .call("POST", &format!("/api/alerts/{id}/ack"), None, &h)
        .await;
    assert_eq!((s1, s2), (200, 200));
}

#[tokio::test]
async fn roles_are_enforced_once_users_exist() {
    let srv = Srv::start().await;
    // Bootstrap: anyone may create the first admin.
    srv.post("/api/users", json!({ "name": "Boss", "role": "admin" }))
        .await;
    let as_ = |name: &'static str| [("x-rfops-actor", name)];

    let (s, v, _) = srv
        .http
        .call(
            "POST",
            "/api/users",
            Some(json!({ "name": "Watcher", "role": "viewer" })),
            &as_("Boss"),
        )
        .await;
    assert_eq!(s, 201, "{v}");
    srv.http
        .call(
            "POST",
            "/api/users",
            Some(json!({ "name": "Left", "role": "a2" })),
            &as_("Boss"),
        )
        .await;

    let (s, _, _) = srv
        .http
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &as_("Nobody"),
        )
        .await;
    assert_eq!(s, 403, "unknown operator");
    let (s, _, _) = srv
        .http
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &as_("Watcher"),
        )
        .await;
    assert_eq!(s, 403, "viewer");
    let (s, _, _) = srv
        .http
        .call(
            "POST",
            "/api/alerts",
            Some(json!({ "severity": "info", "title": "x" })),
            &as_("Left"),
        )
        .await;
    assert_eq!(s, 201, "a2 can raise alerts");
    let (s, _, _) = srv
        .http
        .call(
            "POST",
            "/api/users",
            Some(json!({ "name": "Sneaky", "role": "admin" })),
            &as_("Left"),
        )
        .await;
    assert_eq!(s, 403, "a2 cannot add operators");
}

#[tokio::test]
async fn advance_skips_skipped_songs_and_stops_at_ends() {
    let srv = Srv::start().await;
    let show = srv.post("/api/shows", json!({ "name": "S" })).await["id"]
        .as_i64()
        .unwrap();
    let mut ids = vec![];
    for t in ["A", "B", "C"] {
        ids.push(
            srv.post(&format!("/api/shows/{show}/songs"), json!({ "title": t }))
                .await["id"]
                .as_i64()
                .unwrap(),
        );
    }
    let (s, _, _) = srv
        .http
        .call(
            "PATCH",
            &format!("/api/songs/{}", ids[1]),
            Some(json!({ "skipped": true })),
            &[("x-rfops-actor", "t")],
        )
        .await;
    assert_eq!(s, 200);
    let adv = |dir: &'static str| {
        let srv = &srv;
        async move {
            srv.post(
                &format!("/api/shows/{show}/advance"),
                json!({ "direction": dir }),
            )
            .await["current_song_id"]
                .clone()
        }
    };
    assert_eq!(adv("prev").await, Value::Null);
    assert_eq!(adv("next").await, ids[0]);
    assert_eq!(adv("next").await, ids[2], "B is skipped");
    assert_eq!(adv("next").await, ids[2], "stays at last");
    assert_eq!(adv("prev").await, ids[0]);
    assert_eq!(adv("prev").await, ids[0], "stays at first");
}

#[tokio::test]
async fn demo_seed_is_consistent_and_handoffs_can_be_marked() {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    assert!(seed::seed_demo(&pool).await.unwrap());
    assert!(
        !seed::seed_demo(&pool).await.unwrap(),
        "seeding twice is a no-op"
    );
    let srv = Srv::start_with(pool).await;

    let snap = srv.get("/api/snapshot").await;
    let show = snap["shows"][0]["id"].as_i64().unwrap();
    let snap = srv.get(&format!("/api/snapshot?show_id={show}")).await;
    let s = &snap["show"];
    assert!(
        s["conflicts"]["by_song"].as_array().unwrap().is_empty(),
        "{}",
        s["conflicts"]
    );
    assert!(s["conflicts"]["static_conflicts"]
        .as_array()
        .unwrap()
        .is_empty());
    let plan = s["handoff_plan"].as_array().unwrap();
    let total: usize = plan
        .iter()
        .map(|t| t["handoffs"].as_array().unwrap().len())
        .sum();
    assert!(total >= 8, "demo should exercise handoffs, got {total}");
    assert!(plan
        .iter()
        .flat_map(|t| t["handoffs"].as_array().unwrap())
        .any(|h| h["type"] == "component_change"));
    assert_eq!(
        s["battery"].as_array().unwrap().len(),
        24,
        "every carrier in a kit is listed (11 mic, 10 IEM, 3 guitar)"
    );

    // Mark the first handoff done as an operator who exists in the seed.
    let t0 = &plan[0];
    let h0 = &t0["handoffs"][0];
    let body = json!({
        "from_song": t0["from_song"], "to_song": t0["to_song"],
        "item_id": h0["item_id"], "type": h0["type"], "done": true
    });
    let (st, v, _) = srv
        .http
        .call(
            "POST",
            &format!("/api/shows/{show}/handoffs/done"),
            Some(body.clone()),
            &[("x-rfops-actor", "A2 Stage Left")],
        )
        .await;
    assert_eq!(st, 200, "{v}");
    let snap = srv.get(&format!("/api/snapshot?show_id={show}")).await;
    assert_eq!(snap["show"]["handoffs_done"].as_array().unwrap().len(), 1);
    assert_eq!(snap["show"]["handoffs_done"][0]["done_by"], "A2 Stage Left");

    let mut undo = body;
    undo["done"] = json!(false);
    srv.http
        .call(
            "POST",
            &format!("/api/shows/{show}/handoffs/done"),
            Some(undo),
            &[("x-rfops-actor", "A2 Stage Left")],
        )
        .await;
    let snap = srv.get(&format!("/api/snapshot?show_id={show}")).await;
    assert!(snap["show"]["handoffs_done"].as_array().unwrap().is_empty());
    let _ = &srv.pool;
}

#[tokio::test]
async fn hf2026_template_is_consistent() {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    assert!(seed::seed_hf2026(&pool).await.unwrap());
    assert!(!seed::seed_hf2026(&pool).await.unwrap());
    let srv = Srv::start_with(pool).await;
    let shows = srv.get("/api/snapshot").await["shows"].clone();
    assert_eq!(shows.as_array().unwrap().len(), 2);
    for show in shows.as_array().unwrap() {
        let id = show["id"].as_i64().unwrap();
        let snap = srv.get(&format!("/api/snapshot?show_id={id}")).await;
        let s = &snap["show"];
        assert!(
            s["conflicts"]["by_song"].as_array().unwrap().is_empty(),
            "{}: {}",
            show["name"],
            s["conflicts"]
        );
        let kits = s["kits"].as_array().unwrap();
        let mics: Vec<i64> = kits
            .iter()
            .filter(|k| k["kind"] == "mic")
            .map(|k| k["console_channel"].as_i64().unwrap())
            .collect();
        assert_eq!(mics, (33..=44).collect::<Vec<_>>());
        assert_eq!(kits.iter().filter(|k| k["kind"] == "iem").count(), 12);

        // Helen -> Shosh on RX10 before the handoff song, and back after it.
        let plan = s["handoff_plan"].as_array().unwrap();
        let perf = |n: &str| {
            snap["performers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == n)
                .unwrap()["id"]
                .clone()
        };
        let transfers: Vec<(Value, Value)> = plan
            .iter()
            .flat_map(|t| t["handoffs"].as_array().unwrap().clone())
            .filter(|h| h["type"] == "transfer")
            .map(|h| (h["from_performer"].clone(), h["to_performer"].clone()))
            .collect();
        assert_eq!(
            transfers,
            vec![
                (perf("Helen"), perf("Shosh")),
                (perf("Shosh"), perf("Helen"))
            ]
        );
    }
    let names: Vec<&str> = shows
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"HF Afternoon — Friday") && names.contains(&"HF Afternoon — Saturday"));
}
