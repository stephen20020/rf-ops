//! MIDI endpoints: bridge ingest + heartbeat (shared key), simulate, status/monitor, settings,
//! per-show follow/lock.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post, put},
    Json, Router,
};
use rfops_core::midi::{MidiMsg, Trigger};
use serde::{Deserialize, Serialize};

use crate::{
    db::{log_event, Actor},
    error::{ApiError, ApiResult},
    midi::{self, BridgeInfo, MonitorEntry},
    routes::shows::{fetch_show, Show},
    AppState,
};

pub const BRIDGE_KEY_HEADER: &str = "x-rfops-bridge-key";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/midi", post(ingest))
        .route("/api/midi/heartbeat", post(heartbeat))
        .route("/api/midi/simulate", post(simulate))
        .route("/api/midi/status", get(status))
        .route("/api/midi/settings", put(update_settings))
        .route("/api/midi/key", post(reveal_key))
        .route("/api/midi/key/rotate", post(rotate_key))
        .route("/api/shows/{id}/midi-mode", post(set_mode))
}

async fn check_key(s: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let given = headers
        .get(BRIDGE_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !midi::key_matches(&s.pool, given).await? {
        return Err(ApiError::Unauthorized("wrong or missing bridge key".into()));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Ingest {
    bridge: String,
    port: Option<String>,
    /// Raw MIDI messages, e.g. [[207, 5]] for Program Change 5 on channel 16.
    messages: Vec<Vec<u8>>,
}

#[derive(Serialize)]
pub struct IngestResult {
    outcomes: Vec<String>,
}

async fn ingest(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<Ingest>,
) -> ApiResult<Json<IngestResult>> {
    check_key(&s, &headers).await?;
    midi::heartbeat(&s.pool, &s.midi, &s.hub, &b.bridge, b.port.clone(), None).await?;
    let mut outcomes = Vec::new();
    for raw in &b.messages {
        match MidiMsg::parse(raw) {
            Some(m) => outcomes.push(midi::process(&s.pool, &s.midi, &s.hub, m, "midi").await?),
            None => outcomes.push("not a message RF Ops uses — ignored".into()),
        }
    }
    Ok(Json(IngestResult { outcomes }))
}

#[derive(Debug, Deserialize)]
pub struct Heartbeat {
    bridge: String,
    port: Option<String>,
    #[serde(default)]
    ports: Vec<String>,
}

async fn heartbeat(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<Heartbeat>,
) -> ApiResult<Json<serde_json::Value>> {
    check_key(&s, &headers).await?;
    midi::heartbeat(&s.pool, &s.midi, &s.hub, &b.bridge, b.port, Some(b.ports)).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Send a message as if it came from Playback (testing without the bridge).
async fn simulate(
    State(s): State<AppState>,
    actor: Actor,
    Json(m): Json<MidiMsg>,
) -> ApiResult<Json<IngestResult>> {
    let outcome = midi::process(
        &s.pool,
        &s.midi,
        &s.hub,
        m,
        &format!("simulated by {}", actor.name),
    )
    .await?;
    Ok(Json(IngestResult {
        outcomes: vec![outcome],
    }))
}

#[derive(Serialize)]
pub struct MidiStatus {
    pub bridge: Option<BridgeInfo>,
    pub recent: Vec<MonitorEntry>,
    pub channel: u8,
    pub next_trigger: Option<Trigger>,
    pub prev_trigger: Option<Trigger>,
    pub target_show_id: Option<i64>,
}

pub(crate) async fn current_status(s: &AppState, with_recent: bool) -> ApiResult<MidiStatus> {
    let (bridge, recent) = {
        let st = s.midi.lock().expect("midi state poisoned");
        (
            st.bridge.clone(),
            if with_recent {
                st.recent.iter().cloned().collect()
            } else {
                vec![]
            },
        )
    };
    let channel = midi::setting(&s.pool, "midi.channel")
        .await?
        .and_then(|c| c.parse().ok())
        .unwrap_or(16);
    let next_trigger = midi::setting(&s.pool, "midi.next")
        .await?
        .and_then(|t| Trigger::parse_json(&t));
    let prev_trigger = midi::setting(&s.pool, "midi.prev")
        .await?
        .and_then(|t| Trigger::parse_json(&t));
    let target_show_id = midi::target_show(&s.pool).await?.map(|t| t.0);
    Ok(MidiStatus {
        bridge,
        recent,
        channel,
        next_trigger,
        prev_trigger,
        target_show_id,
    })
}

async fn status(State(s): State<AppState>) -> ApiResult<Json<MidiStatus>> {
    Ok(Json(current_status(&s, true).await?))
}

#[derive(Debug, Deserialize)]
pub struct MidiSettings {
    /// Channel new song triggers use (1–16).
    channel: Option<u8>,
    /// Optional single "next song" / "previous song" cues. `null` clears.
    #[serde(default, with = "crate::double_option")]
    next_trigger: Option<Option<Trigger>>,
    #[serde(default, with = "crate::double_option")]
    prev_trigger: Option<Option<Trigger>>,
}

async fn update_settings(
    State(s): State<AppState>,
    actor: Actor,
    Json(b): Json<MidiSettings>,
) -> ApiResult<Json<MidiStatus>> {
    if !actor.is_admin() {
        return Err(ApiError::Forbidden(
            "only admins can change MIDI settings".into(),
        ));
    }
    if let Some(c) = b.channel {
        if !(1..=16).contains(&c) {
            return Err(ApiError::bad("channel must be 1–16"));
        }
        midi::set_setting(&s.pool, "midi.channel", Some(&c.to_string())).await?;
    }
    for (key, v) in [("midi.next", b.next_trigger), ("midi.prev", b.prev_trigger)] {
        if let Some(v) = v {
            let json = v.map(|t| serde_json::to_string(&t).unwrap_or_default());
            midi::set_setting(&s.pool, key, json.as_deref()).await?;
        }
    }
    let mut tx = s.pool.begin().await?;
    log_event(&mut tx, &actor, "midi.settings", &serde_json::json!({})).await?;
    tx.commit().await?;
    Ok(Json(current_status(&s, false).await?))
}

/// Whether a bridge key exists. The key itself is never shown again after it is made.
async fn reveal_key(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<serde_json::Value>> {
    if !actor.is_admin() {
        return Err(ApiError::Forbidden(
            "only admins can manage the bridge key".into(),
        ));
    }
    Ok(Json(
        serde_json::json!({ "exists": midi::key_exists(&s.pool).await? }),
    ))
}

async fn rotate_key(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<serde_json::Value>> {
    if !actor.is_admin() {
        return Err(ApiError::Forbidden(
            "only admins can change the bridge key".into(),
        ));
    }
    let key = midi::rotate_key(&s.pool).await?;
    let mut tx = s.pool.begin().await?;
    log_event(&mut tx, &actor, "midi.key_rotated", &serde_json::json!({})).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "key": key })))
}

#[derive(Debug, Deserialize)]
pub struct SetMode {
    mode: String,
}

/// Follow MIDI or lock it out, per show. Any operator who can edit may toggle it.
async fn set_mode(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<SetMode>,
) -> ApiResult<Json<Show>> {
    if b.mode != "follow" && b.mode != "lock" {
        return Err(ApiError::bad("mode must be follow or lock"));
    }
    let mut tx = s.pool.begin().await?;
    let mut show = fetch_show(&mut *tx, id).await?;
    sqlx::query("UPDATE shows SET midi_mode = ? WHERE id = ?")
        .bind(&b.mode)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    show.midi_mode = b.mode.clone();
    log_event(
        &mut tx,
        &actor,
        "show.midi_mode",
        &serde_json::json!({ "show_id": id, "mode": b.mode }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(show))
}
