//! Show files and the board, end to end over HTTP (open mode).

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use rfops_server::{app, db, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

const SHOW: &str = r#"
show: HF Afternoon Friday
event: Homestead Fair 2026
timezone: America/Chicago
day: 2026-06-19
set: afternoon
concert: "14:00"
people:
  - Helen
  - { name: Shosh, zone: SR }
  - Judah
devices:
  - { label: Red, kind: mic, channel: "34" }
  - { label: Tan, kind: mic, channel: "35" }
  - { label: RX10, kind: iem }
songs:
  - { title: Walk-in, pc: 1 }
  - title: I Will Follow Jesus
    pc: 5
    passes:
      - "RX10: Helen -> Shosh"
  - { title: Closer, pc: 6 }
plot:
  default:
    Red: Helen
    RX10: Helen
  Closer:
    Tan: Judah
"#;

struct T {
    app: Router,
}

impl T {
    async fn new() -> Self {
        let pool = db::connect("sqlite::memory:").await.unwrap();
        let state = AppState::new(pool).await.unwrap().open_mode().await;
        T { app: app(state) }
    }
    async fn call(&self, method: &str, uri: &str, body: Option<String>, ctype: &str) -> (StatusCode, String) {
        let b = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-rfops-actor", "Sam")
            .header("content-type", ctype);
        let req = b.body(body.map(Body::from).unwrap_or_else(Body::empty)).unwrap();
        let r = self.app.clone().oneshot(req).await.unwrap();
        let st = r.status();
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        (st, String::from_utf8_lossy(&bytes).to_string())
    }
    async fn json(&self, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (st, t) = self
            .call(method, uri, body.map(|b| b.to_string()), "application/json")
            .await;
        (st, serde_json::from_str(&t).unwrap_or(Value::Null))
    }
    async fn yaml(&self, uri: &str, body: &str) -> (StatusCode, Value) {
        let (st, t) = self.call("POST", uri, Some(body.to_string()), "text/yaml").await;
        (st, serde_json::from_str(&t).unwrap_or(Value::Null))
    }
    async fn board(&self, show: i64) -> Value {
        let (st, v) = self.json("GET", &format!("/api/shows/{show}/board"), None).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v
    }
    async fn song(&self, show: i64, title: &str) -> i64 {
        let (_, v) = self.json("GET", "/api/snapshot", None).await;
        let _ = v;
        let (_, snap) = self.json("GET", &format!("/api/snapshot?show_id={show}"), None).await;
        snap["show"]["songs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["title"] == title)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    }
    async fn go(&self, show: i64, song: i64) {
        let (st, v) = self
            .json("POST", &format!("/api/shows/{show}/current"), Some(json!({ "song_id": song })))
            .await;
        assert!(st.is_success(), "{v}");
    }
    /// Tap Done on the first handoff for `device` (by label) in a list ("now" / "coming_up").
    async fn done(&self, show: i64, list: &str, label: &str) -> Value {
        let b = self.board(show).await;
        let id = b["devices"].as_array().unwrap().iter().find(|d| d["label"] == label).unwrap()["id"].clone();
        let h = b["board"][list]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["device"] == id)
            .unwrap_or_else(|| panic!("no {list} handoff for {label}: {b}"))
            .clone();
        let (st, v) = self
            .json(
                "POST",
                &format!("/api/shows/{show}/moves/done"),
                Some(json!({ "device": h["device"], "action": h["action"] })),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        h
    }
}

#[tokio::test]
async fn check_reports_problems_in_plain_english_and_changes_nothing() {
    let t = T::new().await;
    let (st, v) = t.yaml("/api/showfile/check", &SHOW.replace("Tan: Judah", "Tan: Juda")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["ok"], false);
    assert!(v["errors"][0]["message"].as_str().unwrap().contains("Did you mean \"Judah\""), "{v}");
    let (_, shows) = t.json("GET", "/api/shows", None).await;
    assert_eq!(shows.as_array().unwrap().len(), 0);
    // Import refuses with the same problems.
    let (st, _) = t.yaml("/api/showfile/import", &SHOW.replace("Tan: Judah", "Tan: Juda")).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn import_run_the_show_and_round_trip() {
    let t = T::new().await;
    let (st, v) = t.yaml("/api/showfile/import", SHOW).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let show = v["show_id"].as_i64().unwrap();

    // Pre-show: give Red and RX10 to Helen.
    let b = t.board(show).await;
    assert!(b["board"]["now"].as_array().unwrap().is_empty());
    t.done(show, "coming_up", "Red").await;
    t.done(show, "coming_up", "RX10").await;

    let walk = t.song(show, "Walk-in").await;
    let iwfj = t.song(show, "I Will Follow Jesus").await;
    let closer = t.song(show, "Closer").await;
    t.go(show, walk).await;
    t.go(show, iwfj).await;
    let b = t.board(show).await;
    assert!(b["board"]["now"].as_array().unwrap().is_empty(), "{b}");
    let pass = b["board"]["passes"].as_array().unwrap().iter().find(|p| p["prep"] == false).unwrap().clone();
    let (st, v) = t
        .json("POST", &format!("/api/shows/{show}/passes/{}/done", pass["pass"]), None)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Coming up (Closer): RX10 back to Helen, Tan to Judah.
    let b = t.board(show).await;
    let up = b["board"]["coming_up"].to_string();
    assert!(up.contains("\"type\":\"pass\"") && up.contains("\"type\":\"give\""), "{up}");
    // A stale tap is refused.
    let (st, _) = t
        .json(
            "POST",
            &format!("/api/shows/{show}/moves/done"),
            Some(json!({ "device": 999, "action": { "type": "give", "to": 1 } })),
        )
        .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Export → the same plan; re-import keeps the pass mark (same pass id).
    let (st, yaml) = t.call("GET", &format!("/api/shows/{show}/showfile"), None, "text/plain").await;
    assert_eq!(st, StatusCode::OK);
    assert!(yaml.contains("RX10: Helen -> Shosh"), "{yaml}");
    let (st, v) = t.yaml("/api/showfile/import", &yaml).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let b = t.board(show).await;
    assert!(b["board"]["passes"].as_array().unwrap().iter().all(|p| p["prep"] == true || p["pass"] != pass["pass"]), "pass still marked done");

    // Removing a song that was already played is refused.
    let without = yaml.replace("- title: Walk-in\n  pc: 1\n", "");
    let (st, v) = t.yaml("/api/showfile/import", &without).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(v["errors"][0]["message"].as_str().unwrap().contains("already played"));

    // Going back (R3) is a new visit: the pass is due again after Helen gets RX10 back.
    t.go(show, closer).await;
    t.done(show, "now", "RX10").await; // overdue: pass back to Helen
    t.go(show, iwfj).await;
    let b = t.board(show).await;
    assert!(b["board"]["passes"].as_array().unwrap().iter().any(|p| p["prep"] == false), "{b}");
}

#[tokio::test]
async fn device_status_blocks_handing_it_out() {
    let t = T::new().await;
    let (_, v) = t.yaml("/api/showfile/import", SHOW).await;
    let show = v["show_id"].as_i64().unwrap();
    let b = t.board(show).await;
    let red = b["devices"].as_array().unwrap().iter().find(|d| d["label"] == "Red").unwrap()["id"].as_i64().unwrap();
    let (st, _) = t.json("POST", &format!("/api/devices/{red}/status"), Some(json!({ "status": "suspect" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let b = t.board(show).await;
    assert!(!b["board"]["coming_up"].as_array().unwrap().iter().any(|h| h["device"] == red), "suspect Red isn't given out");
    assert!(b["board"]["conflicts"].to_string().contains("unusable_planned"));
}
