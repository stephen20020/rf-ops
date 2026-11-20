//! Endpoints used by the live clients: snapshot, song advance, handoff completion, WebSocket.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use rfops_core::assign::{self, Effective, Transition};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    hub::{max_seq, ws_handler, Event},
    routes::{
        inventory::{Equipment, Performer, User, USER_COLUMNS},
        ops::{compute_battery_status, Alert, BatteryStatus, Checkout},
        shows::{
            conflict_report, fetch_show, load_plan, Assignment, ConflictReport, Kit, Show, Song,
        },
    },
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/ws", get(ws_handler))
        .route("/api/snapshot", get(snapshot))
        .route("/api/shows/{id}/advance", post(advance))
        .route("/api/shows/{id}/handoffs/done", post(mark_handoff))
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct HandoffDone {
    pub from_song: i64,
    pub to_song: i64,
    pub item_id: i64,
    pub handoff_type: String,
    pub done_by: String,
    pub done_at: String,
}

#[derive(Serialize)]
pub struct ShowSnapshot {
    show: Show,
    songs: Vec<Song>,
    kits: Vec<Kit>,
    assignments: Vec<Assignment>,
    /// Effective assignment in the current song (or defaults before the show starts).
    effective: Effective,
    /// Effective assignment in the next song, for "next up" displays.
    next_effective: Option<Effective>,
    next_song_id: Option<i64>,
    handoff_plan: Vec<Transition>,
    upcoming: Vec<Transition>,
    handoffs_done: Vec<HandoffDone>,
    conflicts: ConflictReport,
    battery: Vec<BatteryStatus>,
    /// Swaps in the last 10 minutes, newest first (for the FOH banner after a reconnect).
    recent_swaps: Vec<Event>,
}

#[derive(Serialize)]
pub struct Snapshot {
    seq: i64,
    server_time: String,
    users: Vec<User>,
    performers: Vec<Performer>,
    equipment: Vec<Equipment>,
    shows: Vec<Show>,
    alerts: Vec<Alert>,
    open_checkouts: Vec<Checkout>,
    show: Option<ShowSnapshot>,
}

#[derive(Debug, Deserialize)]
pub struct SnapshotQuery {
    show_id: Option<i64>,
}

/// Everything a client needs to render, in one call. `seq` is read first, so any event with a
/// higher seq may or may not be reflected; clients refetch on events anyway.
async fn snapshot(
    State(s): State<AppState>,
    Query(q): Query<SnapshotQuery>,
) -> ApiResult<Json<Snapshot>> {
    let pool = &s.pool;
    let seq = max_seq(pool).await?;
    let users =
        sqlx::query_as::<_, User>(&format!("SELECT {USER_COLUMNS} FROM users ORDER BY name"))
            .fetch_all(pool)
            .await?;
    let performers = sqlx::query_as::<_, Performer>("SELECT * FROM performers ORDER BY name")
        .fetch_all(pool)
        .await?;
    let equipment = sqlx::query_as::<_, Equipment>("SELECT * FROM equipment ORDER BY kind, label")
        .fetch_all(pool)
        .await?;
    let shows = sqlx::query_as::<_, Show>("SELECT * FROM shows ORDER BY starts_at DESC, id DESC")
        .fetch_all(pool)
        .await?;
    let alerts = sqlx::query_as::<_, Alert>(
        "SELECT * FROM alerts WHERE state <> 'resolved' AND (?1 IS NULL OR show_id IS NULL OR show_id = ?1)
         ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END, opened_at DESC",
    )
    .bind(q.show_id)
    .fetch_all(pool)
    .await?;
    let open_checkouts = sqlx::query_as::<_, Checkout>(
        "SELECT * FROM checkouts WHERE in_at IS NULL ORDER BY out_at",
    )
    .fetch_all(pool)
    .await?;

    let show = match q.show_id {
        None => None,
        Some(id) => {
            let plan = load_plan(pool, id).await?;
            let songs = plan.core_songs();
            let assignments = plan.core_assignments();
            let kits = plan.core_kits();
            let current = plan.show.current_song_id;
            let next_song_id = next_song(&songs, current);
            let handoff_plan = assign::handoff_plan(&songs, &assignments, &kits);
            let upcoming = assign::upcoming_transitions(&handoff_plan, &songs, current, 2);
            let handoffs_done = sqlx::query_as::<_, HandoffDone>(
                "SELECT from_song, to_song, item_id, handoff_type, done_by, done_at FROM handoff_done WHERE show_id = ?",
            )
            .bind(id)
            .fetch_all(pool)
            .await?;
            let battery = compute_battery_status(pool, id, None).await?;
            let conflicts = conflict_report(&plan);
            let since = (chrono::Utc::now() - chrono::Duration::minutes(10))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let recent_swaps = sqlx::query_as::<_, Event>(
                "SELECT * FROM event_log WHERE type = 'swap.performed' AND at >= ?
                 AND json_extract(payload, '$.show_id') = ? ORDER BY seq DESC LIMIT 20",
            )
            .bind(since)
            .bind(id)
            .fetch_all(pool)
            .await?;
            Some(ShowSnapshot {
                effective: assign::effective_assignment(&assignments, current),
                next_effective: next_song_id
                    .map(|n| assign::effective_assignment(&assignments, Some(n))),
                next_song_id,
                handoff_plan,
                upcoming,
                handoffs_done,
                conflicts,
                battery,
                recent_swaps,
                show: plan.show,
                songs: plan.songs,
                kits: plan.kits,
                assignments: plan.assignments,
            })
        }
    };

    Ok(Json(Snapshot {
        seq,
        server_time: now(),
        users,
        performers,
        equipment,
        shows,
        alerts,
        open_checkouts,
        show,
    }))
}

/// Next non-skipped song after `current` (first song when `current` is None).
fn next_song(songs: &[rfops_core::Song], current: Option<i64>) -> Option<i64> {
    let mut ordered: Vec<_> = songs.iter().collect();
    ordered.sort_by_key(|s| (s.position, s.id));
    match current {
        None => ordered.first().map(|s| s.id),
        Some(c) => {
            let i = ordered.iter().position(|s| s.id == c)?;
            ordered.get(i + 1).map(|s| s.id)
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Advance {
    /// "next" or "prev"
    direction: String,
    #[serde(default = "manual")]
    source: String,
}

fn manual() -> String {
    "manual".into()
}

/// Move to the next/previous non-skipped song. At either end the current song is unchanged.
async fn advance(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<Advance>,
) -> ApiResult<Json<Show>> {
    let mut tx = s.pool.begin().await?;
    let mut show = fetch_show(&mut *tx, id).await?;
    let songs: Vec<(i64,)> = sqlx::query_as(
        "SELECT id FROM songs WHERE show_id = ? AND skipped = 0 ORDER BY position, id",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    let ids: Vec<i64> = songs.into_iter().map(|(i,)| i).collect();
    let idx = show
        .current_song_id
        .and_then(|c| ids.iter().position(|&i| i == c));
    let target = match (b.direction.as_str(), idx) {
        ("next", None) => ids.first().copied(),
        ("next", Some(i)) => ids.get(i + 1).copied().or(show.current_song_id),
        ("prev", None) => show.current_song_id,
        ("prev", Some(0)) => show.current_song_id,
        ("prev", Some(i)) => ids.get(i - 1).copied(),
        _ => return Err(ApiError::bad("direction must be next or prev")),
    };
    if target != show.current_song_id {
        sqlx::query("UPDATE shows SET current_song_id = ? WHERE id = ?")
            .bind(target)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_event(
            &mut tx,
            &actor,
            "show.current_song",
            &serde_json::json!({ "show_id": id, "song_id": target, "from_song_id": show.current_song_id, "source": b.source }),
        )
        .await?;
        show.current_song_id = target;
    }
    tx.commit().await?;
    Ok(Json(show))
}

#[derive(Debug, Deserialize)]
pub struct MarkHandoff {
    from_song: Option<i64>,
    to_song: Option<i64>,
    item_id: i64,
    #[serde(rename = "type")]
    handoff_type: String,
    done: bool,
}

const HANDOFF_TYPES: [&str; 4] = ["transfer", "issue", "return", "component_change"];

async fn mark_handoff(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<MarkHandoff>,
) -> ApiResult<Json<serde_json::Value>> {
    if !HANDOFF_TYPES.contains(&b.handoff_type.as_str()) {
        return Err(ApiError::bad("unknown handoff type"));
    }
    let from = b.from_song.unwrap_or(-1);
    let to = b.to_song.unwrap_or(-1);
    let mut tx = s.pool.begin().await?;
    fetch_show(&mut *tx, id).await?;
    if b.done {
        sqlx::query(
            "INSERT OR IGNORE INTO handoff_done (show_id, from_song, to_song, item_id, handoff_type, done_by, done_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(from)
        .bind(to)
        .bind(b.item_id)
        .bind(&b.handoff_type)
        .bind(&actor.name)
        .bind(now())
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "DELETE FROM handoff_done WHERE show_id = ? AND from_song = ? AND to_song = ? AND item_id = ? AND handoff_type = ?",
        )
        .bind(id)
        .bind(from)
        .bind(to)
        .bind(b.item_id)
        .bind(&b.handoff_type)
        .execute(&mut *tx)
        .await?;
    }
    let payload = serde_json::json!({
        "show_id": id, "from_song": b.from_song, "to_song": b.to_song,
        "item_id": b.item_id, "type": b.handoff_type, "done": b.done,
    });
    log_event(&mut tx, &actor, "handoff.marked", &payload).await?;
    tx.commit().await?;
    Ok(Json(payload))
}
