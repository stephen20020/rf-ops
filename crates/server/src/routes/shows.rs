//! Shows, songs, kits, assignments and computed views (effective, conflicts, handoffs).

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, patch, post},
    Json, Router,
};
use rfops_core::{
    assign::{self, Conflict, Effective, Transition},
    Assignment as CoreAssignment, EquipmentStatus, Kit as CoreKit, KitKind, Song as CoreSong,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::{
    db::{log_event, Actor},
    double_option,
    error::{ApiError, ApiResult},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/shows", get(list_shows).post(create_show))
        .route(
            "/api/shows/{id}",
            get(get_show_full).patch(update_show).delete(delete_show),
        )
        .route("/api/shows/{id}/duplicate", post(duplicate_show))
        .route("/api/shows/{id}/current", post(set_current_song))
        .route("/api/shows/{id}/songs", post(create_song))
        .route("/api/shows/{id}/songs/reorder", post(reorder_songs))
        .route("/api/songs/{id}", patch(update_song).delete(delete_song))
        .route("/api/shows/{id}/kits", post(create_kit))
        .route("/api/kits/{id}", patch(update_kit).delete(delete_kit))
        .route("/api/shows/{id}/assignments", post(create_assignment))
        .route("/api/assignments/{id}", delete(delete_assignment))
        .route(
            "/api/shows/{id}/assignments/cell",
            axum::routing::put(set_cell),
        )
        .route("/api/shows/{id}/effective", get(effective))
        .route("/api/shows/{id}/conflicts", get(conflicts))
        .route("/api/shows/{id}/handoffs", get(handoffs))
        .route("/api/shows/{id}/upcoming", get(upcoming))
}

// ---------------- rows ----------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Show {
    pub id: i64,
    pub name: String,
    pub starts_at: Option<String>,
    pub call_time: Option<String>,
    pub current_song_id: Option<i64>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Song {
    pub id: i64,
    pub show_id: i64,
    pub position: i64,
    pub title: String,
    pub midi_trigger: Option<String>,
    pub skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Kit {
    pub id: i64,
    pub show_id: i64,
    pub kind: String,
    pub name: String,
    pub base_id: Option<i64>,
    pub carrier_id: Option<i64>,
    pub element_id: Option<i64>,
    pub console_channel: Option<i64>,
    pub console_channel_name: Option<String>,
    pub dca: Option<String>,
    pub iem_mix: Option<String>,
    pub is_spare: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Assignment {
    pub id: i64,
    pub show_id: i64,
    pub performer_id: i64,
    pub kind: String,
    pub kit_id: Option<i64>,
    pub song_id: Option<i64>,
}

const MODES: [&str; 3] = ["setup", "rehearsal", "show"];

fn kit_kind(s: &str) -> ApiResult<KitKind> {
    KitKind::parse(s)
        .ok_or_else(|| ApiError::bad(format!("kit kind must be mic, iem or guitar, got '{s}'")))
}

// ---------------- loading helpers ----------------

pub(crate) async fn fetch_show<'e, E: sqlx::SqliteExecutor<'e>>(e: E, id: i64) -> ApiResult<Show> {
    sqlx::query_as::<_, Show>("SELECT * FROM shows WHERE id = ?")
        .bind(id)
        .fetch_optional(e)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Everything needed by the pure planning functions.
pub(crate) struct Plan {
    pub show: Show,
    pub songs: Vec<Song>,
    pub kits: Vec<Kit>,
    pub assignments: Vec<Assignment>,
    pub statuses: HashMap<i64, EquipmentStatus>,
}

impl Plan {
    /// Skipped songs are excluded from planning.
    pub fn core_songs(&self) -> Vec<CoreSong> {
        self.songs
            .iter()
            .filter(|s| !s.skipped)
            .map(|s| CoreSong {
                id: s.id,
                position: s.position,
                title: s.title.clone(),
            })
            .collect()
    }

    pub fn core_kits(&self) -> HashMap<i64, CoreKit> {
        self.kits
            .iter()
            .filter_map(|k| {
                Some((
                    k.id,
                    CoreKit {
                        id: k.id,
                        kind: KitKind::parse(&k.kind)?,
                        name: k.name.clone(),
                        base_id: k.base_id,
                        carrier_id: k.carrier_id,
                        element_id: k.element_id,
                        console_channel: k.console_channel,
                        console_channel_name: k.console_channel_name.clone(),
                        dca: k.dca.clone(),
                        iem_mix: k.iem_mix.clone(),
                        is_spare: k.is_spare,
                    },
                ))
            })
            .collect()
    }

    pub fn core_assignments(&self) -> Vec<CoreAssignment> {
        self.assignments
            .iter()
            .filter_map(|a| {
                Some(CoreAssignment {
                    id: a.id,
                    performer_id: a.performer_id,
                    kind: KitKind::parse(&a.kind)?,
                    kit_id: a.kit_id,
                    song_id: a.song_id,
                })
            })
            .collect()
    }
}

pub(crate) async fn load_plan(pool: &SqlitePool, show_id: i64) -> ApiResult<Plan> {
    let show = fetch_show(pool, show_id).await?;
    let songs =
        sqlx::query_as::<_, Song>("SELECT * FROM songs WHERE show_id = ? ORDER BY position, id")
            .bind(show_id)
            .fetch_all(pool)
            .await?;
    let kits = sqlx::query_as::<_, Kit>("SELECT * FROM kits WHERE show_id = ? ORDER BY id")
        .bind(show_id)
        .fetch_all(pool)
        .await?;
    let assignments =
        sqlx::query_as::<_, Assignment>("SELECT * FROM assignments WHERE show_id = ? ORDER BY id")
            .bind(show_id)
            .fetch_all(pool)
            .await?;
    let statuses: Vec<(i64, String)> = sqlx::query_as(
        "SELECT e.id, e.status FROM equipment e WHERE e.id IN (
            SELECT base_id FROM kits WHERE show_id = ?1 UNION
            SELECT carrier_id FROM kits WHERE show_id = ?1 UNION
            SELECT element_id FROM kits WHERE show_id = ?1)",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    let statuses = statuses
        .into_iter()
        .filter_map(|(id, s)| Some((id, EquipmentStatus::parse(&s)?)))
        .collect();
    Ok(Plan {
        show,
        songs,
        kits,
        assignments,
        statuses,
    })
}

// ---------------- shows ----------------

#[derive(Debug, Deserialize)]
pub struct NewShow {
    pub name: String,
    pub starts_at: Option<String>,
    pub call_time: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ShowPatch {
    pub name: Option<String>,
    #[serde(default, with = "double_option")]
    pub starts_at: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub call_time: Option<Option<String>>,
    pub mode: Option<String>,
}

fn check_time(t: &Option<String>) -> ApiResult<()> {
    if let Some(t) = t {
        chrono::DateTime::parse_from_rfc3339(t)
            .map_err(|_| ApiError::bad(format!("'{t}' is not an RFC 3339 timestamp")))?;
    }
    Ok(())
}

async fn list_shows(State(s): State<AppState>) -> ApiResult<Json<Vec<Show>>> {
    Ok(Json(
        sqlx::query_as::<_, Show>("SELECT * FROM shows ORDER BY starts_at DESC, id DESC")
            .fetch_all(&s.pool)
            .await?,
    ))
}

async fn create_show(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewShow>,
) -> ApiResult<(StatusCode, Json<Show>)> {
    if n.name.trim().is_empty() {
        return Err(ApiError::bad("name is required"));
    }
    check_time(&n.starts_at)?;
    check_time(&n.call_time)?;
    let mut tx = s.pool.begin().await?;
    let id = sqlx::query("INSERT INTO shows (name, starts_at, call_time) VALUES (?, ?, ?)")
        .bind(n.name.trim())
        .bind(&n.starts_at)
        .bind(&n.call_time)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    let show = fetch_show(&mut *tx, id).await?;
    log_event(&mut tx, &actor, "show.created", &show).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(show)))
}

#[derive(Serialize)]
pub struct ShowFull {
    show: Show,
    songs: Vec<Song>,
    kits: Vec<Kit>,
    assignments: Vec<Assignment>,
}

async fn get_show_full(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ShowFull>> {
    let p = load_plan(&s.pool, id).await?;
    Ok(Json(ShowFull {
        show: p.show,
        songs: p.songs,
        kits: p.kits,
        assignments: p.assignments,
    }))
}

async fn update_show(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(patch): Json<ShowPatch>,
) -> ApiResult<Json<Show>> {
    let mut tx = s.pool.begin().await?;
    let mut show = fetch_show(&mut *tx, id).await?;
    if let Some(v) = patch.name {
        if v.trim().is_empty() {
            return Err(ApiError::bad("name is required"));
        }
        show.name = v.trim().into();
    }
    if let Some(v) = patch.starts_at {
        check_time(&v)?;
        show.starts_at = v;
    }
    if let Some(v) = patch.call_time {
        check_time(&v)?;
        show.call_time = v;
    }
    if let Some(v) = patch.mode {
        if !MODES.contains(&v.as_str()) {
            return Err(ApiError::bad(format!("mode must be one of {MODES:?}")));
        }
        show.mode = v;
    }
    sqlx::query("UPDATE shows SET name=?, starts_at=?, call_time=?, mode=? WHERE id=?")
        .bind(&show.name)
        .bind(&show.starts_at)
        .bind(&show.call_time)
        .bind(&show.mode)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "show.updated", &show).await?;
    tx.commit().await?;
    Ok(Json(show))
}

async fn delete_show(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let show = fetch_show(&mut *tx, id).await?;
    sqlx::query("DELETE FROM shows WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "show.deleted", &show).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct DuplicateShow {
    pub name: String,
    pub starts_at: Option<String>,
    pub call_time: Option<String>,
}

/// Copy songs, kits and assignments into a new show (templates / repeat services).
async fn duplicate_show(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(d): Json<DuplicateShow>,
) -> ApiResult<(StatusCode, Json<Show>)> {
    check_time(&d.starts_at)?;
    check_time(&d.call_time)?;
    let src = load_plan(&s.pool, id).await?;
    let mut tx = s.pool.begin().await?;
    let new_id = sqlx::query("INSERT INTO shows (name, starts_at, call_time) VALUES (?, ?, ?)")
        .bind(d.name.trim())
        .bind(&d.starts_at)
        .bind(&d.call_time)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    let mut song_map = HashMap::new();
    for song in &src.songs {
        let nid = sqlx::query(
            "INSERT INTO songs (show_id, position, title, midi_trigger) VALUES (?, ?, ?, ?)",
        )
        .bind(new_id)
        .bind(song.position)
        .bind(&song.title)
        .bind(&song.midi_trigger)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        song_map.insert(song.id, nid);
    }
    let mut kit_map = HashMap::new();
    for k in &src.kits {
        let nid = sqlx::query(
            "INSERT INTO kits (show_id, kind, name, base_id, carrier_id, element_id, console_channel, console_channel_name, dca, iem_mix, is_spare)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id)
        .bind(&k.kind)
        .bind(&k.name)
        .bind(k.base_id)
        .bind(k.carrier_id)
        .bind(k.element_id)
        .bind(k.console_channel)
        .bind(&k.console_channel_name)
        .bind(&k.dca)
        .bind(&k.iem_mix)
        .bind(k.is_spare)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        kit_map.insert(k.id, nid);
    }
    for a in &src.assignments {
        sqlx::query("INSERT INTO assignments (show_id, performer_id, kind, kit_id, song_id) VALUES (?, ?, ?, ?, ?)")
            .bind(new_id)
            .bind(a.performer_id)
            .bind(&a.kind)
            .bind(a.kit_id.and_then(|k| kit_map.get(&k).copied()))
            .bind(a.song_id.and_then(|s| song_map.get(&s).copied()))
            .execute(&mut *tx)
            .await?;
    }
    let show = fetch_show(&mut *tx, new_id).await?;
    log_event(
        &mut tx,
        &actor,
        "show.duplicated",
        &serde_json::json!({ "from": id, "show": show }),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(show)))
}

#[derive(Debug, Deserialize)]
pub struct SetCurrent {
    pub song_id: Option<i64>,
    /// "manual" or "midi"
    #[serde(default = "manual")]
    pub source: String,
}

fn manual() -> String {
    "manual".into()
}

async fn set_current_song(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(b): Json<SetCurrent>,
) -> ApiResult<Json<Show>> {
    let mut tx = s.pool.begin().await?;
    let mut show = fetch_show(&mut *tx, id).await?;
    if let Some(song_id) = b.song_id {
        let ok: Option<i64> =
            sqlx::query_scalar("SELECT id FROM songs WHERE id = ? AND show_id = ?")
                .bind(song_id)
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if ok.is_none() {
            return Err(ApiError::bad("song does not belong to this show"));
        }
    }
    sqlx::query("UPDATE shows SET current_song_id = ? WHERE id = ?")
        .bind(b.song_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    show.current_song_id = b.song_id;
    log_event(
        &mut tx,
        &actor,
        "show.current_song",
        &serde_json::json!({ "show_id": id, "song_id": b.song_id, "source": b.source }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(show))
}

// ---------------- songs ----------------

#[derive(Debug, Deserialize)]
pub struct NewSong {
    pub title: String,
    /// Defaults to the end of the setlist.
    pub position: Option<i64>,
    pub midi_trigger: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SongPatch {
    pub title: Option<String>,
    #[serde(default, with = "double_option")]
    pub midi_trigger: Option<Option<serde_json::Value>>,
    pub skipped: Option<bool>,
}

async fn fetch_song<'e, E: sqlx::SqliteExecutor<'e>>(e: E, id: i64) -> ApiResult<Song> {
    sqlx::query_as::<_, Song>("SELECT * FROM songs WHERE id = ?")
        .bind(id)
        .fetch_optional(e)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn create_song(
    State(s): State<AppState>,
    actor: Actor,
    Path(show_id): Path<i64>,
    Json(n): Json<NewSong>,
) -> ApiResult<(StatusCode, Json<Song>)> {
    if n.title.trim().is_empty() {
        return Err(ApiError::bad("title is required"));
    }
    let mut tx = s.pool.begin().await?;
    fetch_show(&mut *tx, show_id).await?;
    let max: Option<i64> = sqlx::query_scalar("SELECT MAX(position) FROM songs WHERE show_id = ?")
        .bind(show_id)
        .fetch_one(&mut *tx)
        .await?;
    let end = max.unwrap_or(0) + 1;
    let position = match n.position {
        Some(p) if p >= 1 && p < end => {
            sqlx::query(
                "UPDATE songs SET position = position + 1 WHERE show_id = ? AND position >= ?",
            )
            .bind(show_id)
            .bind(p)
            .execute(&mut *tx)
            .await?;
            p
        }
        _ => end,
    };
    let id = sqlx::query(
        "INSERT INTO songs (show_id, position, title, midi_trigger) VALUES (?, ?, ?, ?)",
    )
    .bind(show_id)
    .bind(position)
    .bind(n.title.trim())
    .bind(n.midi_trigger.map(|v| v.to_string()))
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let song = fetch_song(&mut *tx, id).await?;
    log_event(&mut tx, &actor, "song.created", &song).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(song)))
}

async fn update_song(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(p): Json<SongPatch>,
) -> ApiResult<Json<Song>> {
    let mut tx = s.pool.begin().await?;
    let mut song = fetch_song(&mut *tx, id).await?;
    if let Some(v) = p.title {
        if v.trim().is_empty() {
            return Err(ApiError::bad("title is required"));
        }
        song.title = v.trim().into();
    }
    if let Some(v) = p.midi_trigger {
        song.midi_trigger = v.map(|j| j.to_string());
    }
    if let Some(v) = p.skipped {
        song.skipped = v;
    }
    sqlx::query("UPDATE songs SET title=?, midi_trigger=?, skipped=? WHERE id=?")
        .bind(&song.title)
        .bind(&song.midi_trigger)
        .bind(song.skipped)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "song.updated", &song).await?;
    tx.commit().await?;
    Ok(Json(song))
}

async fn delete_song(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let song = fetch_song(&mut *tx, id).await?;
    sqlx::query("UPDATE shows SET current_song_id = NULL WHERE current_song_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM songs WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE songs SET position = position - 1 WHERE show_id = ? AND position > ?")
        .bind(song.show_id)
        .bind(song.position)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "song.deleted", &song).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct Reorder {
    pub song_ids: Vec<i64>,
}

async fn reorder_songs(
    State(s): State<AppState>,
    actor: Actor,
    Path(show_id): Path<i64>,
    Json(r): Json<Reorder>,
) -> ApiResult<Json<Vec<Song>>> {
    let mut tx = s.pool.begin().await?;
    let mut existing: Vec<i64> = sqlx::query_scalar("SELECT id FROM songs WHERE show_id = ?")
        .bind(show_id)
        .fetch_all(&mut *tx)
        .await?;
    let mut requested = r.song_ids.clone();
    existing.sort_unstable();
    requested.sort_unstable();
    if existing != requested {
        return Err(ApiError::bad(
            "song_ids must list every song in the show exactly once",
        ));
    }
    for (i, id) in r.song_ids.iter().enumerate() {
        sqlx::query("UPDATE songs SET position = ? WHERE id = ?")
            .bind(i as i64 + 1)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    log_event(
        &mut tx,
        &actor,
        "songs.reordered",
        &serde_json::json!({ "show_id": show_id, "song_ids": r.song_ids }),
    )
    .await?;
    let songs =
        sqlx::query_as::<_, Song>("SELECT * FROM songs WHERE show_id = ? ORDER BY position")
            .bind(show_id)
            .fetch_all(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(Json(songs))
}

// ---------------- kits ----------------

#[derive(Debug, Deserialize)]
pub struct NewKit {
    pub kind: String,
    pub name: String,
    pub base_id: Option<i64>,
    pub carrier_id: Option<i64>,
    pub element_id: Option<i64>,
    pub console_channel: Option<i64>,
    pub console_channel_name: Option<String>,
    pub dca: Option<String>,
    pub iem_mix: Option<String>,
    #[serde(default)]
    pub is_spare: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct KitPatch {
    pub name: Option<String>,
    #[serde(default, with = "double_option")]
    pub base_id: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub carrier_id: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub element_id: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub console_channel: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub console_channel_name: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub dca: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub iem_mix: Option<Option<String>>,
    pub is_spare: Option<bool>,
}

async fn fetch_kit<'e, E: sqlx::SqliteExecutor<'e>>(e: E, id: i64) -> ApiResult<Kit> {
    sqlx::query_as::<_, Kit>("SELECT * FROM kits WHERE id = ?")
        .bind(id)
        .fetch_optional(e)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Validate that each slot holds a compatible equipment kind.
async fn validate_kit_slots(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    kind: KitKind,
    base: Option<i64>,
    carrier: Option<i64>,
    element: Option<i64>,
) -> ApiResult<()> {
    use rfops_core::EquipmentKind as E;
    let (base_ok, carrier_ok, element_ok): (&[E], &[E], &[E]) = match kind {
        KitKind::Mic => (
            &[E::ReceiverChannel],
            &[E::HandheldTx, E::BodypackTx],
            &[E::Capsule, E::Headset, E::Lav],
        ),
        KitKind::Iem => (&[E::IemTxChannel, E::IemTx], &[E::IemPack], &[]),
        KitKind::Guitar => (&[E::GuitarReceiver], &[E::GuitarPack], &[]),
    };
    for (slot, id, allowed) in [
        ("base_id", base, base_ok),
        ("carrier_id", carrier, carrier_ok),
        ("element_id", element, element_ok),
    ] {
        let Some(id) = id else { continue };
        let eq_kind: Option<String> = sqlx::query_scalar("SELECT kind FROM equipment WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
        let eq_kind = eq_kind
            .ok_or_else(|| ApiError::bad(format!("{slot}: equipment {id} does not exist")))?;
        let parsed = E::parse(&eq_kind);
        if !parsed.is_some_and(|k| allowed.contains(&k)) {
            return Err(ApiError::bad(format!(
                "{slot}: {eq_kind} is not valid in a {} kit (allowed: {})",
                kind.as_str(),
                allowed
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
    Ok(())
}

async fn create_kit(
    State(s): State<AppState>,
    actor: Actor,
    Path(show_id): Path<i64>,
    Json(n): Json<NewKit>,
) -> ApiResult<(StatusCode, Json<Kit>)> {
    let kind = kit_kind(&n.kind)?;
    if n.name.trim().is_empty() {
        return Err(ApiError::bad("name is required"));
    }
    let mut tx = s.pool.begin().await?;
    fetch_show(&mut *tx, show_id).await?;
    validate_kit_slots(&mut tx, kind, n.base_id, n.carrier_id, n.element_id).await?;
    let id = sqlx::query(
        "INSERT INTO kits (show_id, kind, name, base_id, carrier_id, element_id, console_channel, console_channel_name, dca, iem_mix, is_spare)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(show_id)
    .bind(kind.as_str())
    .bind(n.name.trim())
    .bind(n.base_id)
    .bind(n.carrier_id)
    .bind(n.element_id)
    .bind(n.console_channel)
    .bind(&n.console_channel_name)
    .bind(&n.dca)
    .bind(&n.iem_mix)
    .bind(n.is_spare)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let kit = fetch_kit(&mut *tx, id).await?;
    log_event(&mut tx, &actor, "kit.created", &kit).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(kit)))
}

async fn update_kit(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(p): Json<KitPatch>,
) -> ApiResult<Json<Kit>> {
    let mut tx = s.pool.begin().await?;
    let mut k = fetch_kit(&mut *tx, id).await?;
    if let Some(v) = p.name {
        if v.trim().is_empty() {
            return Err(ApiError::bad("name is required"));
        }
        k.name = v.trim().into();
    }
    if let Some(v) = p.base_id {
        k.base_id = v;
    }
    if let Some(v) = p.carrier_id {
        k.carrier_id = v;
    }
    if let Some(v) = p.element_id {
        k.element_id = v;
    }
    if let Some(v) = p.console_channel {
        k.console_channel = v;
    }
    if let Some(v) = p.console_channel_name {
        k.console_channel_name = v;
    }
    if let Some(v) = p.dca {
        k.dca = v;
    }
    if let Some(v) = p.iem_mix {
        k.iem_mix = v;
    }
    if let Some(v) = p.is_spare {
        k.is_spare = v;
    }
    validate_kit_slots(
        &mut tx,
        kit_kind(&k.kind)?,
        k.base_id,
        k.carrier_id,
        k.element_id,
    )
    .await?;
    sqlx::query(
        "UPDATE kits SET name=?, base_id=?, carrier_id=?, element_id=?, console_channel=?, console_channel_name=?, dca=?, iem_mix=?, is_spare=? WHERE id=?",
    )
    .bind(&k.name)
    .bind(k.base_id)
    .bind(k.carrier_id)
    .bind(k.element_id)
    .bind(k.console_channel)
    .bind(&k.console_channel_name)
    .bind(&k.dca)
    .bind(&k.iem_mix)
    .bind(k.is_spare)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    log_event(&mut tx, &actor, "kit.updated", &k).await?;
    tx.commit().await?;
    Ok(Json(k))
}

async fn delete_kit(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let k = fetch_kit(&mut *tx, id).await?;
    sqlx::query("DELETE FROM kits WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "kit.deleted", &k).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------- assignments ----------------

#[derive(Debug, Deserialize)]
pub struct NewAssignment {
    pub performer_id: i64,
    pub kind: String,
    pub kit_id: Option<i64>,
    pub song_id: Option<i64>,
}

#[derive(Serialize)]
pub struct AssignmentResult {
    assignment: Assignment,
    /// Conflicts in the affected song (or defaults) after this change. Saved regardless, so the
    /// editor can show them; resolving is the operator's call.
    conflicts: Vec<Conflict>,
}

async fn create_assignment(
    State(s): State<AppState>,
    actor: Actor,
    Path(show_id): Path<i64>,
    Json(n): Json<NewAssignment>,
) -> ApiResult<(StatusCode, Json<AssignmentResult>)> {
    let kind = kit_kind(&n.kind)?;
    if n.kit_id.is_none() && n.song_id.is_none() {
        return Err(ApiError::bad("a default assignment needs a kit_id; use song_id with kit_id null to remove a kit for one song"));
    }
    let mut tx = s.pool.begin().await?;
    fetch_show(&mut *tx, show_id).await?;
    if let Some(kit_id) = n.kit_id {
        let k = fetch_kit(&mut *tx, kit_id)
            .await
            .map_err(|_| ApiError::bad("kit does not exist"))?;
        if k.show_id != show_id {
            return Err(ApiError::bad("kit belongs to another show"));
        }
        if k.kind != kind.as_str() {
            return Err(ApiError::bad(format!(
                "kit is a {} kit, not {}",
                k.kind,
                kind.as_str()
            )));
        }
    }
    if let Some(song_id) = n.song_id {
        let song = fetch_song(&mut *tx, song_id)
            .await
            .map_err(|_| ApiError::bad("song does not exist"))?;
        if song.show_id != show_id {
            return Err(ApiError::bad("song belongs to another show"));
        }
    }
    let id = sqlx::query("INSERT INTO assignments (show_id, performer_id, kind, kit_id, song_id) VALUES (?, ?, ?, ?, ?)")
        .bind(show_id)
        .bind(n.performer_id)
        .bind(kind.as_str())
        .bind(n.kit_id)
        .bind(n.song_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    let a = sqlx::query_as::<_, Assignment>("SELECT * FROM assignments WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "assignment.created", &a).await?;
    tx.commit().await?;
    let plan = load_plan(&s.pool, show_id).await?;
    let conflicts = assign::conflicts_for(
        &plan.core_assignments(),
        &plan.core_kits(),
        &plan.statuses,
        n.song_id,
    );
    Ok((
        StatusCode::CREATED,
        Json(AssignmentResult {
            assignment: a,
            conflicts,
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct SetCell {
    pub performer_id: i64,
    pub kind: String,
    /// `None` = show default column.
    pub song_id: Option<i64>,
    /// Kits for this cell. Empty + song = "none in this song"; empty + default = no default.
    #[serde(default)]
    pub kit_ids: Vec<i64>,
    /// Song cells only: remove the override so the default applies.
    #[serde(default)]
    pub inherit: bool,
}

/// Replace one (performer, kind, song) cell of the assignment grid atomically.
async fn set_cell(
    State(s): State<AppState>,
    actor: Actor,
    Path(show_id): Path<i64>,
    Json(c): Json<SetCell>,
) -> ApiResult<Json<AssignmentCellResult>> {
    let kind = kit_kind(&c.kind)?;
    if c.inherit && c.song_id.is_none() {
        return Err(ApiError::bad("inherit only applies to song cells"));
    }
    let mut tx = s.pool.begin().await?;
    fetch_show(&mut *tx, show_id).await?;
    if let Some(song_id) = c.song_id {
        let song = fetch_song(&mut *tx, song_id)
            .await
            .map_err(|_| ApiError::bad("song does not exist"))?;
        if song.show_id != show_id {
            return Err(ApiError::bad("song belongs to another show"));
        }
    }
    for &kit_id in &c.kit_ids {
        let k = fetch_kit(&mut *tx, kit_id)
            .await
            .map_err(|_| ApiError::bad("kit does not exist"))?;
        if k.show_id != show_id || k.kind != kind.as_str() {
            return Err(ApiError::bad(format!(
                "kit {} is not a {} kit in this show",
                k.name,
                kind.as_str()
            )));
        }
    }
    sqlx::query(
        "DELETE FROM assignments WHERE show_id = ? AND performer_id = ? AND kind = ? AND song_id IS ?",
    )
    .bind(show_id)
    .bind(c.performer_id)
    .bind(kind.as_str())
    .bind(c.song_id)
    .execute(&mut *tx)
    .await?;
    let rows: Vec<Option<i64>> = if c.inherit {
        vec![]
    } else if c.kit_ids.is_empty() {
        if c.song_id.is_some() {
            vec![None]
        } else {
            vec![]
        }
    } else {
        c.kit_ids.iter().map(|k| Some(*k)).collect()
    };
    for kit in rows {
        sqlx::query(
            "INSERT INTO assignments (show_id, performer_id, kind, kit_id, song_id) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(show_id)
        .bind(c.performer_id)
        .bind(kind.as_str())
        .bind(kit)
        .bind(c.song_id)
        .execute(&mut *tx)
        .await?;
    }
    let payload = serde_json::json!({
        "show_id": show_id, "performer_id": c.performer_id, "kind": kind.as_str(),
        "song_id": c.song_id, "kit_ids": c.kit_ids, "inherit": c.inherit,
    });
    log_event(&mut tx, &actor, "assignment.cell_set", &payload).await?;
    tx.commit().await?;
    let plan = load_plan(&s.pool, show_id).await?;
    let conflicts = assign::conflicts_for(
        &plan.core_assignments(),
        &plan.core_kits(),
        &plan.statuses,
        c.song_id,
    );
    Ok(Json(AssignmentCellResult { conflicts }))
}

#[derive(Serialize)]
pub struct AssignmentCellResult {
    conflicts: Vec<Conflict>,
}

async fn delete_assignment(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let a = sqlx::query_as::<_, Assignment>("SELECT * FROM assignments WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    sqlx::query("DELETE FROM assignments WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "assignment.deleted", &a).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------- computed views ----------------

#[derive(Debug, Deserialize)]
pub struct SongQuery {
    song_id: Option<i64>,
}

async fn effective(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<SongQuery>,
) -> ApiResult<Json<Effective>> {
    let plan = load_plan(&s.pool, id).await?;
    let song = q.song_id.or(plan.show.current_song_id);
    Ok(Json(assign::effective_assignment(
        &plan.core_assignments(),
        song,
    )))
}

#[derive(Serialize)]
pub struct SongConflicts {
    /// `None` = show defaults.
    song_id: Option<i64>,
    conflicts: Vec<Conflict>,
}

#[derive(Serialize)]
pub struct ConflictReport {
    static_conflicts: Vec<Conflict>,
    by_song: Vec<SongConflicts>,
}

async fn conflicts(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ConflictReport>> {
    let plan = load_plan(&s.pool, id).await?;
    Ok(Json(conflict_report(&plan)))
}

pub(crate) fn conflict_report(plan: &Plan) -> ConflictReport {
    let a = plan.core_assignments();
    let k = plan.core_kits();
    let mut by_song = Vec::new();
    for song_id in std::iter::once(None).chain(plan.core_songs().iter().map(|s| Some(s.id))) {
        let c = assign::conflicts_for(&a, &k, &plan.statuses, song_id);
        if !c.is_empty() {
            by_song.push(SongConflicts {
                song_id,
                conflicts: c,
            });
        }
    }
    ConflictReport {
        static_conflicts: assign::kind_mismatches(&a, &k),
        by_song,
    }
}

async fn handoffs(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<Transition>>> {
    let plan = load_plan(&s.pool, id).await?;
    Ok(Json(assign::handoff_plan(
        &plan.core_songs(),
        &plan.core_assignments(),
        &plan.core_kits(),
    )))
}

#[derive(Debug, Deserialize)]
pub struct UpcomingQuery {
    count: Option<usize>,
}

async fn upcoming(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<UpcomingQuery>,
) -> ApiResult<Json<Vec<Transition>>> {
    let plan = load_plan(&s.pool, id).await?;
    let songs = plan.core_songs();
    let full = assign::handoff_plan(&songs, &plan.core_assignments(), &plan.core_kits());
    Ok(Json(assign::upcoming_transitions(
        &full,
        &songs,
        plan.show.current_song_id,
        q.count.unwrap_or(2),
    )))
}
