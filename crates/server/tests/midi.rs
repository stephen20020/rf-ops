//! Phase 4: MIDI from Playback via the bridge.

use std::time::{Duration, Instant};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use rfops_server::{app, db, midi, seed, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

struct T {
    app: Router,
    state: AppState,
    pool: sqlx::SqlitePool,
    key: String,
}

impl T {
    async fn demo() -> Self {
        let pool = db::connect("sqlite::memory:").await.unwrap();
        seed::seed_demo(&pool).await.unwrap();
        let state = AppState::new(pool.clone()).await.unwrap();
        let key = midi::ensure_key(&pool).await.unwrap();
        T {
            app: app(state.clone()),
            state,
            pool,
            key,
        }
    }

    async fn call(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (StatusCode, Value) {
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
        let s = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// Send raw MIDI as the bridge. Returns the outcomes.
    async fn midi(&self, msgs: Value) -> Vec<String> {
        let (s, v) = self
            .call(
                "POST",
                "/api/midi",
                Some(json!({ "bridge": "Playback Mac", "port": "IAC Driver Bus 1", "messages": msgs })),
                &[("x-rfops-bridge-key", &self.key)],
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_str().unwrap().to_string())
            .collect()
    }

    async fn as_admin(&self, method: &str, uri: &str, body: Value) -> Value {
        let (s, v) = self
            .call(method, uri, Some(body), &[("x-rfops-actor", "Admin")])
            .await;
        assert!(s.is_success(), "{method} {uri}: {s} {v}");
        v
    }

    async fn current(&self) -> Option<String> {
        sqlx::query_scalar(
            "SELECT so.title FROM shows sh JOIN songs so ON so.id = sh.current_song_id LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap()
    }

    async fn show(&self) -> i64 {
        sqlx::query_scalar("SELECT id FROM shows LIMIT 1")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

/// Program Change on channel 16 (status 0xCF).
fn pc16(v: u8) -> Value {
    json!([[0xCF, v]])
}

#[tokio::test]
async fn bridge_needs_the_key() {
    let t = T::demo().await;
    let body = json!({ "bridge": "x", "messages": [[0xCF, 1]] });
    let (s, _) = t.call("POST", "/api/midi", Some(body.clone()), &[]).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = t
        .call(
            "POST",
            "/api/midi",
            Some(body),
            &[("x-rfops-bridge-key", "nope")],
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(t.current().await, None, "nothing changed");

    // Only admins can see the key.
    let (s, _) = t
        .call(
            "POST",
            "/api/midi/key",
            None,
            &[("x-rfops-actor", "A2 Stage Left")],
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let k = t.as_admin("POST", "/api/midi/key", json!({})).await;
    assert_eq!(k["key"], t.key);
}

#[tokio::test]
async fn program_change_moves_to_song_and_is_logged_as_midi() {
    let t = T::demo().await;
    // Demo songs listen for PC 1..8 on channel 16; PC 3 = "Song 2".
    let out = t.midi(pc16(3)).await;
    assert_eq!(out, vec!["→ Song 2"]);
    assert_eq!(t.current().await.as_deref(), Some("Song 2"));
    let source: String = sqlx::query_scalar(
        "SELECT json_extract(payload, '$.source') FROM event_log WHERE type = 'show.current_song' ORDER BY seq DESC LIMIT 1",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(source, "midi");

    // A repeat inside 500 ms is dropped; the same cue again later says "already on".
    assert_eq!(t.midi(pc16(3)).await, vec!["duplicate — ignored"]);
    tokio::time::sleep(Duration::from_millis(550)).await;
    assert_eq!(t.midi(pc16(3)).await, vec!["already on Song 2"]);

    // Other channels and non-cue messages are ignored.
    assert_eq!(
        t.midi(json!([[0xC0, 5]])).await,
        vec!["not a song cue — ignored"]
    );
    assert_eq!(
        t.midi(json!([[0xF8]])).await,
        vec!["not a message RF Ops uses — ignored"]
    );
    assert_eq!(t.current().await.as_deref(), Some("Song 2"));
}

#[tokio::test]
async fn lock_ignores_midi_and_follow_resumes() {
    let t = T::demo().await;
    let show = t.show().await;
    t.as_admin(
        "POST",
        &format!("/api/shows/{show}/midi-mode"),
        json!({ "mode": "lock" }),
    )
    .await;
    let out = t.midi(pc16(5)).await;
    assert_eq!(out, vec!["LOCKED — would go to Song 4 — Ballad"]);
    assert_eq!(t.current().await, None);
    // Manual changes still work while locked.
    t.as_admin(
        "POST",
        &format!("/api/shows/{show}/advance"),
        json!({ "direction": "next" }),
    )
    .await;
    assert_eq!(t.current().await.as_deref(), Some("Walk-in / Welcome"));

    t.as_admin(
        "POST",
        &format!("/api/shows/{show}/midi-mode"),
        json!({ "mode": "follow" }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(550)).await;
    assert_eq!(t.midi(pc16(5)).await, vec!["→ Song 4 — Ballad"]);

    let (s, _) = t
        .call(
            "POST",
            &format!("/api/shows/{show}/midi-mode"),
            Some(json!({ "mode": "maybe" })),
            &[("x-rfops-actor", "Admin")],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unmatched_cue_raises_one_warning_and_skipped_songs_do_not_match() {
    let t = T::demo().await;
    assert_eq!(
        t.midi(pc16(99)).await,
        vec!["no matching song — warning raised"]
    );
    tokio::time::sleep(Duration::from_millis(550)).await;
    t.midi(pc16(99)).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alerts WHERE source = 'midi' AND state <> 'resolved'",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "deduplicated");
    assert_eq!(t.current().await, None);

    // Skip "Song 3" (PC 4): its cue now matches nothing.
    let s3: i64 = sqlx::query_scalar("SELECT id FROM songs WHERE title = 'Song 3'")
        .fetch_one(&t.pool)
        .await
        .unwrap();
    t.as_admin(
        "PATCH",
        &format!("/api/songs/{s3}"),
        json!({ "skipped": true }),
    )
    .await;
    assert_eq!(
        t.midi(pc16(4)).await,
        vec!["no matching song — warning raised"]
    );
}

#[tokio::test]
async fn next_and_prev_cues() {
    let t = T::demo().await;
    t.as_admin(
        "PUT",
        "/api/midi/settings",
        json!({ "next_trigger": { "type": "note_on", "channel": 16, "note": 60 }, "prev_trigger": { "type": "note_on", "channel": 16, "note": 59 } }),
    )
    .await;
    assert_eq!(
        t.midi(json!([[0x9F, 60, 100]])).await,
        vec!["→ Walk-in / Welcome"]
    );
    tokio::time::sleep(Duration::from_millis(550)).await;
    assert_eq!(
        t.midi(json!([[0x9F, 60, 100]])).await,
        vec!["→ Song 1 — Opener"]
    );
    assert_eq!(
        t.midi(json!([[0x9F, 59, 100]])).await,
        vec!["→ Walk-in / Welcome"]
    );
    // Note-off (velocity 0) does nothing.
    tokio::time::sleep(Duration::from_millis(550)).await;
    assert_eq!(
        t.midi(json!([[0x9F, 60, 0]])).await,
        vec!["not a message RF Ops uses — ignored"]
    );

    let (s, _) = t
        .call(
            "PUT",
            "/api/midi/settings",
            Some(json!({ "channel": 17 })),
            &[("x-rfops-actor", "Admin")],
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = t
        .call(
            "PUT",
            "/api/midi/settings",
            Some(json!({ "channel": 1 })),
            &[("x-rfops-actor", "FOH")],
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn simulate_needs_an_operator_and_status_shows_the_monitor() {
    let t = T::demo().await;
    let msg = json!({ "type": "program_change", "channel": 16, "value": 2 });
    let (s, _) = t
        .call(
            "POST",
            "/api/midi/simulate",
            Some(msg.clone()),
            &[("x-rfops-actor", "Viewer")],
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = t
        .call(
            "POST",
            "/api/midi/simulate",
            Some(msg),
            &[("x-rfops-actor", "FOH")],
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["outcomes"][0], "→ Song 1 — Opener");

    let (_, st) = t.call("GET", "/api/midi/status", None, &[]).await;
    assert_eq!(st["recent"][0]["message"], "Program Change 2 · ch 16");
    assert_eq!(st["recent"][0]["source"], "simulated by FOH");
    assert_eq!(st["channel"], 16);
    assert!(st["bridge"].is_null(), "no bridge yet");

    let (s, _) = t
        .call(
            "POST",
            "/api/midi/heartbeat",
            Some(json!({ "bridge": "Playback Mac", "port": "IAC Driver Bus 1", "ports": ["IAC Driver Bus 1", "PlayAUDIO1U"] })),
            &[("x-rfops-bridge-key", &t.key)],
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, snap) = t.call("GET", "/api/snapshot", None, &[]).await;
    assert_eq!(snap["midi"]["bridge"]["online"], true);
    assert_eq!(snap["midi"]["bridge"]["ports"][1], "PlayAUDIO1U");
    let online: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM event_log WHERE type = 'midi.bridge_online'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(online, 1);
}

#[tokio::test]
async fn bridge_offline_alert_in_show_mode_and_recovery() {
    let t = T::demo().await;
    let show = t.show().await;
    t.midi(json!([])).await; // heartbeat via ingest: bridge online
    t.as_admin(
        "PATCH",
        &format!("/api/shows/{show}"),
        json!({ "mode": "show" }),
    )
    .await;

    // Pretend the last heartbeat was 20 s ago.
    {
        let mut st = t.state.midi.lock().unwrap();
        st.bridge.as_mut().unwrap().last_seen =
            Instant::now().checked_sub(Duration::from_secs(20)).unwrap();
    }
    midi::watch_once(&t.pool, &t.state.midi, &t.state.hub)
        .await
        .unwrap();
    let open: Vec<String> = sqlx::query_scalar(
        "SELECT title FROM alerts WHERE dedupe_key LIKE 'midi-bridge:%' AND state <> 'resolved'",
    )
    .fetch_all(&t.pool)
    .await
    .unwrap();
    assert_eq!(open, vec!["MIDI bridge offline"]);
    let off: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM event_log WHERE type = 'midi.bridge_offline'")
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert_eq!(off, 1);
    midi::watch_once(&t.pool, &t.state.midi, &t.state.hub)
        .await
        .unwrap();

    // Bridge returns: alert resolves.
    t.midi(json!([])).await;
    midi::watch_once(&t.pool, &t.state.midi, &t.state.hub)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alerts WHERE dedupe_key LIKE 'midi-bridge:%' AND state <> 'resolved'",
    )
    .fetch_one(&t.pool)
    .await
    .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn midi_follows_the_show_in_show_mode() {
    let t = T::demo().await;
    let show = t.show().await;
    // A newer rehearsal show exists, but the show-mode show wins.
    let copy = t
        .as_admin(
            "POST",
            &format!("/api/shows/{show}/duplicate"),
            json!({ "name": "Copy" }),
        )
        .await;
    let copy_id = copy["id"].as_i64().unwrap();
    t.as_admin(
        "PATCH",
        &format!("/api/shows/{copy_id}"),
        json!({ "mode": "rehearsal" }),
    )
    .await;
    t.as_admin(
        "PATCH",
        &format!("/api/shows/{show}"),
        json!({ "mode": "show" }),
    )
    .await;
    t.midi(pc16(2)).await;
    let cur: Option<i64> = sqlx::query_scalar("SELECT current_song_id FROM shows WHERE id = ?")
        .bind(show)
        .fetch_one(&t.pool)
        .await
        .unwrap();
    let cur_copy: Option<i64> =
        sqlx::query_scalar("SELECT current_song_id FROM shows WHERE id = ?")
            .bind(copy_id)
            .fetch_one(&t.pool)
            .await
            .unwrap();
    assert!(cur.is_some());
    assert!(cur_copy.is_none());
}
