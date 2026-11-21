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
    /// Returns the handoff, with the event seq (for Undo) under "seq".
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
        let mut h = h;
        h["seq"] = v["seq"].clone();
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

#[tokio::test]
async fn hf2026_example_file_imports_on_the_hf_seed() {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    rfops_server::seed::seed_hf2026(&pool).await.unwrap();
    let state = AppState::new(pool).await.unwrap().open_mode().await;
    let t = T { app: app(state) };
    let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/hf2026-afternoon-friday.yaml")).unwrap();
    let (st, v) = t.yaml("/api/showfile/import", &file).await;
    assert_eq!(st, StatusCode::OK, "updates the seeded show of the same name: {v}");
    let show = v["show_id"].as_i64().unwrap();
    assert_eq!(show, 1);
    let b = t.board(show).await;
    let rx3 = b["devices"].as_array().unwrap().iter().find(|d| d["label"] == "RX3").unwrap().clone();
    assert_eq!(rx3["kind"], "iem");
    // Pre-show: 6 mics (Seth has Red for Awesome God) + 12 IEM packs to give out.
    assert_eq!(b["board"]["coming_up"].as_array().unwrap().len(), 18, "{}", b["board"]["coming_up"]);
    assert!(b["board"]["conflicts"].as_array().unwrap().is_empty());
    // Red goes from Seth to Judah between Awesome God and Joy.
    t.done(show, "coming_up", "Red").await;
    let ag = t.song(show, "Awesome God").await;
    t.go(show, ag).await;
    let b = t.board(show).await;
    assert!(b["board"]["coming_up"].to_string().contains("\"type\":\"pass\""), "{}", b["board"]["coming_up"]);
    // Saturday imports too (Chandler on Blue for the last 3 songs).
    let sat = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/hf2026-afternoon-saturday.yaml")).unwrap();
    let (st, v) = t.yaml("/api/showfile/import", &sat).await;
    assert!(st.is_success(), "{v}");
    assert!(v["warnings"].as_array().unwrap().is_empty(), "{v}");
}

const SWAP_SHOW: &str = r#"
show: Swap test
people: [Helen, Judah, Shosh]
devices:
  - { label: Red, kind: mic, channel: "34" }
  - { label: Bpack3, kind: mic, channel: "43", spare: true }
  - { label: Tan, kind: mic, channel: "35" }
songs:
  - { title: One }
  - { title: Two }
  - { title: Three }
plot:
  default:
    Red: Helen
  Three:
    Tan: Judah
"#;

async fn dev_id(t: &T, show: i64, label: &str) -> i64 {
    let b = t.board(show).await;
    b["devices"].as_array().unwrap().iter().find(|d| d["label"] == label).unwrap()["id"].as_i64().unwrap()
}

#[tokio::test]
async fn swap_until_fixed_full_flow_and_return() {
    let t = T::new().await;
    let (_, v) = t.yaml("/api/showfile/import", SWAP_SHOW).await;
    let show = v["show_id"].as_i64().unwrap();
    t.done(show, "coming_up", "Red").await;
    let one = t.song(show, "One").await;
    t.go(show, one).await;
    let red = dev_id(&t, show, "Red").await;
    let bp = dev_id(&t, show, "Bpack3").await;

    // Candidates: the spare first.
    let (_, c) = t.json("GET", &format!("/api/shows/{show}/devices/{red}/candidates"), None).await;
    assert_eq!(c["candidates"][0]["label"], "Bpack3", "{c}");
    assert_eq!(c["candidates"][0]["tier"]["tier"], "spare");

    // Swap (R1 default until fixed): recorded → both devices off the handoff lists.
    let (st, sw) = t.json("POST", &format!("/api/shows/{show}/swaps"), Some(json!({ "failed": red, "replacement": bp }))).await;
    assert_eq!(st, StatusCode::CREATED, "{sw}");
    let sid = sw["id"].as_i64().unwrap();
    let b = t.board(show).await;
    assert!(b["board"]["held_by_swap"].as_array().unwrap().len() == 2);
    // FOH can't confirm before In hand.
    let (st, _) = t.json("POST", &format!("/api/swaps/{sid}/foh-done"), None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = t.json("POST", &format!("/api/swaps/{sid}/in-hand"), None).await;
    assert_eq!(st, StatusCode::OK);
    let b = t.board(show).await;
    let tile = |label: &str| b["devices"].as_array().unwrap().iter().find(|d| d["label"] == label).unwrap().clone();
    assert_eq!(tile("Bpack3")["usage"]["usage"], "on_spare_for");
    assert_eq!(tile("Red")["usage"]["usage"], "suspect");
    assert!(b["board"]["now"].as_array().unwrap().is_empty(), "{}", b["board"]["now"]);
    let (st, _) = t.json("POST", &format!("/api/swaps/{sid}/foh-done"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, list) = t.json("GET", &format!("/api/shows/{show}/swaps"), None).await;
    assert_eq!(list[0]["state"], "foh_confirmed");
    assert_eq!(list[0]["failed_channel"], "34");
    assert_eq!(list[0]["replacement_channel"], "43");

    // Next song: Helen stays on the spare (until fixed).
    let two = t.song(show, "Two").await;
    t.go(show, two).await;
    let b = t.board(show).await;
    assert!(b["board"]["now"].as_array().unwrap().is_empty());

    // Red is fixed → Return (two steps) → Red back to Helen, Bpack3 to stock.
    let (st, _) = t.json("POST", &format!("/api/swaps/{sid}/return"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = t.json("POST", &format!("/api/swaps/{sid}/return-in-hand"), None).await;
    assert_eq!(st, StatusCode::OK);
    let b = t.board(show).await;
    let tile = |label: &str| b["devices"].as_array().unwrap().iter().find(|d| d["label"] == label).unwrap().clone();
    assert_eq!(tile("Red")["usage"]["usage"], "in_use");
    assert_eq!(tile("Bpack3")["usage"]["usage"], "spare");
    assert!(b["board"]["now"].as_array().unwrap().is_empty(), "{}", b["board"]["now"]);
}

#[tokio::test]
async fn undo_battery_plot_edit_pull_and_count_in() {
    let t = T::new().await;
    let (_, v) = t.yaml("/api/showfile/import", SWAP_SHOW).await;
    let show = v["show_id"].as_i64().unwrap();
    let red = dev_id(&t, show, "Red").await;
    let tan = dev_id(&t, show, "Tan").await;

    // Done, then Undo within 10 s.
    let h = t.done(show, "coming_up", "Red").await;
    let (st, v) = t.json("POST", &format!("/api/shows/{show}/undo"), Some(json!({ "seq": h["seq"] }))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let b = t.board(show).await;
    assert!(b["board"]["coming_up"].to_string().contains("give"), "back to needing a give");

    // Battery check counts for this show.
    let (st, _) = t.json("POST", &format!("/api/devices/{red}/battery"), Some(json!({ "result": "fresh" }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, bat) = t.json("GET", &format!("/api/shows/{show}/batteries"), None).await;
    assert_eq!(bat["devices"][red.to_string()]["result"], "fresh");

    // Plot edit from any screen (R7): Tan to Shosh by default.
    let (_, snap) = t.json("GET", "/api/snapshot", None).await;
    let shosh = snap["performers"].as_array().unwrap().iter().find(|p| p["name"] == "Shosh").unwrap()["id"].clone();
    let (st, _) = t.json("PUT", &format!("/api/shows/{show}/plot"), Some(json!({ "device": tan, "song": null, "performer": shosh }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let b = t.board(show).await;
    assert!(b["board"]["coming_up"].as_array().unwrap().iter().any(|h| h["device"] == tan));

    // Pull mid-song: no instant collect; collected at the next song.
    t.done(show, "coming_up", "Red").await;
    let one = t.song(show, "One").await;
    t.go(show, one).await;
    let (st, p) = t.json("POST", &format!("/api/shows/{show}/swaps"), Some(json!({ "kind": "pull", "failed": red }))).await;
    assert_eq!(st, StatusCode::CREATED, "{p}");
    let b = t.board(show).await;
    assert!(!b["board"]["now"].as_array().unwrap().iter().any(|h| h["device"] == red));

    // Count-in close marks anything still out as missing.
    let (st, v) = t.json("POST", &format!("/api/shows/{show}/count-in/close"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["missing"].to_string().contains("Red"), "{v}");
}

/// The owner's case: a database seeded with the *demo* data (HH4, BP1…), not HF 2026. The
/// example must still import: its own devices and missing transmitters are added.
#[tokio::test]
async fn hf2026_example_imports_on_a_demo_database_too() {
    let pool = db::connect("sqlite::memory:").await.unwrap();
    rfops_server::seed::seed_demo(&pool).await.unwrap();
    let state = AppState::new(pool).await.unwrap().open_mode().await;
    let t = T { app: app(state) };
    let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/hf2026-afternoon-friday.yaml")).unwrap();
    let (st, v) = t.yaml("/api/showfile/check", &file).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["ok"], true, "{v}");
    assert!(v["warnings"].to_string().contains("will be added"), "{v}");
    let (st, v) = t.yaml("/api/showfile/import", &file).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let show = v["show_id"].as_i64().unwrap();
    let b = t.board(show).await;
    let tile = |l: &str| b["devices"].as_array().unwrap().iter().find(|d| d["label"] == l).cloned();
    assert!(tile("Red").is_some() && tile("GLXD1+-1").is_some());
    assert_eq!(b["board"]["coming_up"].as_array().unwrap().len(), 18, "{}", b["board"]["coming_up"]);
    // Importing again changes nothing new (idempotent).
    let (st, v) = t.yaml("/api/showfile/import", &file).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["warnings"].as_array().unwrap().is_empty(), "{v}");
}

#[tokio::test]
async fn preshow_checklist_ticks_itself_and_report_tells_the_story() {
    let t = T::new().await;
    let (_, v) = t.yaml("/api/showfile/import", SWAP_SHOW).await;
    let show = v["show_id"].as_i64().unwrap();
    let get = |v: &Value, k: &str| v["items"].as_array().unwrap().iter().find(|i| i["key"] == k).unwrap().clone();

    let (_, c) = t.json("GET", &format!("/api/shows/{show}/checklist"), None).await;
    assert_eq!(get(&c, "batteries")["ok"], false, "{c}");
    assert!(get(&c, "batteries")["detail"].as_str().unwrap().contains("Not checked: Red"));
    assert_eq!(get(&c, "handed_out")["ok"], false);
    assert_eq!(get(&c, "plan")["ok"], true);
    assert!(c["items"].as_array().unwrap().iter().all(|i| i["key"] != "midi"), "no MIDI in this show");

    // Batteries and hand-out tick themselves.
    for l in ["Red", "Tan", "Bpack3"] {
        let d = dev_id(&t, show, l).await;
        t.json("POST", &format!("/api/devices/{d}/battery"), Some(json!({ "result": "ok" }))).await;
    }
    t.done(show, "coming_up", "Red").await;
    let (_, c) = t.json("GET", &format!("/api/shows/{show}/checklist"), None).await;
    assert_eq!(get(&c, "batteries")["ok"], true, "{c}");
    assert_eq!(get(&c, "spares")["ok"], true);
    assert_eq!(get(&c, "handed_out")["ok"], true);

    // Manual ticks, custom items; auto items can't be ticked by hand.
    let (st, _) = t.json("POST", &format!("/api/shows/{show}/checklist/tick"), Some(json!({ "key": "wifi", "ticked": true }))).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = t.json("POST", &format!("/api/shows/{show}/checklist/tick"), Some(json!({ "key": "batteries", "ticked": true }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    t.json("POST", &format!("/api/shows/{show}/checklist/custom"), Some(json!({ "label": "Choir mics on stands" }))).await;
    let (_, c) = t.json("GET", &format!("/api/shows/{show}/checklist"), None).await;
    assert_eq!(get(&c, "wifi")["ticked_by"], "Sam");
    assert!(c["items"].to_string().contains("Choir mics on stands"));

    // Run a bit of the show: a swap and a problem, then the report.
    let (st, _) = t.json("PATCH", &format!("/api/shows/{show}"), Some(json!({ "mode": "show" }))).await;
    assert!(st.is_success());
    let one = t.song(show, "One").await;
    t.go(show, one).await;
    let red = dev_id(&t, show, "Red").await;
    let bp = dev_id(&t, show, "Bpack3").await;
    let (_, sw) = t.json("POST", &format!("/api/shows/{show}/swaps"), Some(json!({ "failed": red, "replacement": bp, "component": "element" }))).await;
    t.json("POST", &format!("/api/swaps/{}/in-hand", sw["id"]), None).await;
    t.json("POST", "/api/alerts", Some(json!({ "severity": "critical", "title": "Red (Helen): No audio", "show_id": show }))).await;
    let (st, r) = t.json("GET", &format!("/api/shows/{show}/post-show"), None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["played"][0]["song"], "One");
    assert_eq!(r["not_played"].as_array().unwrap().len(), 2);
    assert_eq!(r["swaps"][0]["failed"], "Red");
    assert_eq!(r["swaps"][0]["component"], "element");
    let steps: Vec<String> = r["swaps"][0]["timeline"].as_array().unwrap().iter().map(|s| s["step"].as_str().unwrap().to_string()).collect();
    assert_eq!(steps, vec!["recorded", "in_hand"]);
    assert_eq!(r["problems"][0]["title"], "Red (Helen): No audio");
    assert_eq!(r["batteries"]["counts"]["ok"], 3);
    assert!(r["attention"].to_string().contains("suspect"), "Red is suspect after the swap: {r}");
}

#[tokio::test]
async fn reset_after_rehearsal_clears_the_run_and_allows_reimport() {
    let t = T::new().await;
    let (_, v) = t.yaml("/api/showfile/import", SWAP_SHOW).await;
    let show = v["show_id"].as_i64().unwrap();
    t.done(show, "coming_up", "Red").await;
    let one = t.song(show, "One").await;
    t.go(show, one).await;
    let red = dev_id(&t, show, "Red").await;
    let bp = dev_id(&t, show, "Bpack3").await;
    let (_, sw) = t.json("POST", &format!("/api/shows/{show}/swaps"), Some(json!({ "failed": red, "replacement": bp }))).await;
    t.json("POST", &format!("/api/swaps/{}/in-hand", sw["id"]), None).await;
    // Removing a played song is refused, and the message points at reset.
    let without = SWAP_SHOW.replace("  - { title: One }\n", "");
    let (st, v) = t.yaml("/api/showfile/import", &without).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(v["errors"][0]["message"].as_str().unwrap().contains("reset the run"), "{v}");

    let (st, v) = t.json("POST", &format!("/api/shows/{show}/reset-run"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let b = t.board(show).await;
    assert!(b["board"]["current_song"].is_null());
    let tile = |l: &str| b["devices"].as_array().unwrap().iter().find(|d| d["label"] == l).unwrap().clone();
    assert_eq!(tile("Red")["holder"]["kind"], "stock");
    assert_eq!(tile("Bpack3")["holder"]["kind"], "stock");
    assert_ne!(tile("Red")["usage"]["usage"], "suspect", "back in service");
    let (_, swaps) = t.json("GET", &format!("/api/shows/{show}/swaps"), None).await;
    assert!(swaps.as_array().unwrap().is_empty());
    // Now the song can be removed.
    let (st, v) = t.yaml("/api/showfile/import", &without).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}
