//! Live show operations: emergency swap to a spare, and the post-show report.

use std::collections::HashSet;

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use rfops_core::assign;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    routes::{
        ops::Alert,
        shows::{load_plan, Kit, Show},
    },
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/shows/{id}/swap", post(swap))
        .route("/api/shows/{id}/report", get(report))
}

#[derive(Debug, Deserialize)]
pub struct SwapRequest {
    pub performer_id: i64,
    pub from_kit_id: i64,
    pub to_kit_id: i64,
    /// "song" (current song only) or "rest_of_show" (current and all later songs).
    #[serde(default = "rest")]
    pub scope: String,
    #[serde(default)]
    pub reason: String,
    pub alert_id: Option<i64>,
    /// Mark the failed item faulty (default: the from-kit's transmitter/pack).
    #[serde(default)]
    pub mark_faulty: bool,
    pub faulty_item_id: Option<i64>,
}

fn rest() -> String {
    "rest_of_show".into()
}

#[derive(Debug, Serialize)]
struct KitRef {
    id: i64,
    name: String,
    console_channel: Option<i64>,
    console_channel_name: Option<String>,
    iem_mix: Option<String>,
}

impl From<&Kit> for KitRef {
    fn from(k: &Kit) -> Self {
        KitRef {
            id: k.id,
            name: k.name.clone(),
            console_channel: k.console_channel,
            console_channel_name: k.console_channel_name.clone(),
            iem_mix: k.iem_mix.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SwapResult {
    swap_id: i64,
    songs_changed: Vec<i64>,
    faulty_item_id: Option<i64>,
}

/// Move a performer from one kit to another (usually a spare) for the current song or the rest of
/// the show, in one transaction. Allowed in show mode.
async fn swap(
    State(s): State<AppState>,
    actor: Actor,
    Path(show_id): Path<i64>,
    Json(r): Json<SwapRequest>,
) -> ApiResult<Json<SwapResult>> {
    if r.scope != "song" && r.scope != "rest_of_show" {
        return Err(ApiError::bad("scope must be song or rest_of_show"));
    }
    if r.from_kit_id == r.to_kit_id {
        return Err(ApiError::bad("choose a different kit to swap to"));
    }
    let plan = load_plan(&s.pool, show_id).await?;
    let find = |id: i64| plan.kits.iter().find(|k| k.id == id);
    let from = find(r.from_kit_id).ok_or_else(|| ApiError::bad("from kit is not in this show"))?;
    let to = find(r.to_kit_id).ok_or_else(|| ApiError::bad("spare kit is not in this show"))?;
    if from.kind != to.kind {
        return Err(ApiError::bad(format!(
            "cannot swap a {} kit for a {} kit",
            from.kind, to.kind
        )));
    }
    let performer: String = sqlx::query_scalar("SELECT name FROM performers WHERE id = ?")
        .bind(r.performer_id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| ApiError::bad("performer does not exist"))?;

    let songs = plan.core_songs();
    let mut ordered: Vec<_> = songs.iter().collect();
    ordered.sort_by_key(|s| (s.position, s.id));
    let current = plan.show.current_song_id;
    let start = current.and_then(|c| ordered.iter().position(|s| s.id == c));
    let affected: Vec<i64> = match (r.scope.as_str(), start) {
        ("song", Some(i)) => vec![ordered[i].id],
        ("song", None) => return Err(ApiError::bad("there is no current song; use rest_of_show")),
        (_, Some(i)) => ordered[i..].iter().map(|s| s.id).collect(),
        (_, None) => ordered.iter().map(|s| s.id).collect(),
    };

    let assignments = plan.core_assignments();
    let kits = plan.core_kits();
    let to_items: HashSet<i64> = kits[&to.id].items().collect();
    let mut changes: Vec<(i64, Vec<i64>)> = Vec::new();
    for &song in &affected {
        let eff = assign::effective_assignment(&assignments, Some(song));
        // The spare must be free: nobody else may hold the kit or any of its items.
        for (&p, ks) in &eff {
            if p == r.performer_id {
                continue;
            }
            for k in ks {
                if *k == to.id
                    || kits
                        .get(k)
                        .is_some_and(|kk| kk.items().any(|i| to_items.contains(&i)))
                {
                    let other: String =
                        sqlx::query_scalar("SELECT name FROM performers WHERE id = ?")
                            .bind(p)
                            .fetch_one(&s.pool)
                            .await?;
                    return Err(ApiError::Conflict(format!(
                        "{} is in use by {other} in {}",
                        to.name,
                        plan.songs
                            .iter()
                            .find(|x| x.id == song)
                            .map(|x| x.title.as_str())
                            .unwrap_or("a later song")
                    )));
                }
            }
        }
        let mine: Vec<i64> = eff
            .get(&r.performer_id)
            .map(|ks| {
                ks.iter()
                    .copied()
                    .filter(|k| kits.get(k).is_some_and(|kk| kk.kind.as_str() == from.kind))
                    .collect()
            })
            .unwrap_or_default();
        if mine.contains(&from.id) {
            let mut new: Vec<i64> = mine.into_iter().filter(|k| *k != from.id).collect();
            if !new.contains(&to.id) {
                new.push(to.id);
            }
            changes.push((song, new));
        }
    }
    if changes.is_empty() {
        return Err(ApiError::bad(format!(
            "{performer} does not have {} in the affected songs",
            from.name
        )));
    }

    let mut tx = s.pool.begin().await?;
    for (song, kit_ids) in &changes {
        sqlx::query(
            "DELETE FROM assignments WHERE show_id = ? AND performer_id = ? AND kind = ? AND song_id = ?",
        )
        .bind(show_id)
        .bind(r.performer_id)
        .bind(&from.kind)
        .bind(song)
        .execute(&mut *tx)
        .await?;
        for k in kit_ids {
            sqlx::query(
                "INSERT INTO assignments (show_id, performer_id, kind, kit_id, song_id) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(show_id)
            .bind(r.performer_id)
            .bind(&from.kind)
            .bind(k)
            .bind(song)
            .execute(&mut *tx)
            .await?;
        }
    }
    let faulty = if r.mark_faulty {
        r.faulty_item_id.or(from.carrier_id)
    } else {
        None
    };
    if let Some(item) = faulty {
        sqlx::query("UPDATE equipment SET status = 'faulty' WHERE id = ?")
            .bind(item)
            .execute(&mut *tx)
            .await?;
    }
    let t = now();
    if let Some(alert_id) = r.alert_id {
        // Someone acted on it: acknowledge if still open.
        sqlx::query(
            "UPDATE alerts SET state = 'acknowledged', acked_at = ?, acked_by = ? WHERE id = ? AND state = 'open'",
        )
        .bind(&t)
        .bind(&actor.name)
        .bind(alert_id)
        .execute(&mut *tx)
        .await?;
    }
    let swap_id = sqlx::query(
        "INSERT INTO swaps (show_id, performer_id, from_kit_id, to_kit_id, scope, song_id, reason, alert_id, at, by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(show_id)
    .bind(r.performer_id)
    .bind(from.id)
    .bind(to.id)
    .bind(&r.scope)
    .bind(current)
    .bind(&r.reason)
    .bind(r.alert_id)
    .bind(&t)
    .bind(&actor.name)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let songs_changed: Vec<i64> = changes.iter().map(|(s, _)| *s).collect();
    log_event(
        &mut tx,
        &actor,
        "swap.performed",
        &serde_json::json!({
            "show_id": show_id,
            "swap_id": swap_id,
            "performer_id": r.performer_id,
            "performer_name": performer,
            "from_kit": KitRef::from(from),
            "to_kit": KitRef::from(to),
            "scope": r.scope,
            "songs_changed": songs_changed,
            "faulty_item_id": faulty,
            "reason": r.reason,
            "alert_id": r.alert_id,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(SwapResult {
        swap_id,
        songs_changed,
        faulty_item_id: faulty,
    }))
}

// ---------------- report ----------------

#[derive(Debug, Serialize, FromRow)]
pub struct SwapRow {
    id: i64,
    at: String,
    by: String,
    performer: String,
    from_kit: Option<String>,
    to_kit: String,
    scope: String,
    song: Option<String>,
    reason: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct BatteryRow {
    checked_at: String,
    checked_by: String,
    label: String,
    result: String,
    battery_type: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct FaultyRow {
    id: i64,
    label: String,
    kind: String,
    model: String,
    notes: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct SongChange {
    at: String,
    actor: String,
    song: Option<String>,
    source: Option<String>,
}

#[derive(Serialize)]
pub struct Report {
    show: Show,
    generated_at: String,
    alerts: Vec<Alert>,
    swaps: Vec<SwapRow>,
    battery_checks: Vec<BatteryRow>,
    faulty_items: Vec<FaultyRow>,
    song_changes: Vec<SongChange>,
    handoffs_done: i64,
}

async fn report(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Report>> {
    let pool = &s.pool;
    let plan = load_plan(pool, id).await?;
    let alerts =
        sqlx::query_as::<_, Alert>("SELECT * FROM alerts WHERE show_id = ? ORDER BY opened_at")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let swaps = sqlx::query_as::<_, SwapRow>(
        "SELECT sw.id, sw.at, sw.by, p.name AS performer, fk.name AS from_kit, tk.name AS to_kit,
                sw.scope, so.title AS song, sw.reason
         FROM swaps sw
         JOIN performers p ON p.id = sw.performer_id
         LEFT JOIN kits fk ON fk.id = sw.from_kit_id
         JOIN kits tk ON tk.id = sw.to_kit_id
         LEFT JOIN songs so ON so.id = sw.song_id
         WHERE sw.show_id = ? ORDER BY sw.at",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    // Checks on this show's packs since call time (or the last 24 h if no call time is set).
    let since = plan.show.call_time.clone().unwrap_or_else(|| {
        (chrono::Utc::now() - chrono::Duration::hours(24))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    });
    let battery_checks = sqlx::query_as::<_, BatteryRow>(
        "SELECT bc.checked_at, bc.checked_by, e.label, bc.result, bc.battery_type
         FROM battery_checks bc JOIN equipment e ON e.id = bc.equipment_id
         WHERE bc.equipment_id IN (SELECT carrier_id FROM kits WHERE show_id = ?)
           AND bc.checked_at >= ?
         ORDER BY bc.checked_at",
    )
    .bind(id)
    .bind(&since)
    .fetch_all(pool)
    .await?;
    let faulty_items = sqlx::query_as::<_, FaultyRow>(
        "SELECT id, label, kind, model, notes FROM equipment WHERE status = 'faulty' AND id IN (
            SELECT base_id FROM kits WHERE show_id = ?1 UNION
            SELECT carrier_id FROM kits WHERE show_id = ?1 UNION
            SELECT element_id FROM kits WHERE show_id = ?1)
         ORDER BY label",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let song_changes = sqlx::query_as::<_, SongChange>(
        "SELECT ev.at, ev.actor, so.title AS song, json_extract(ev.payload, '$.source') AS source
         FROM event_log ev
         LEFT JOIN songs so ON so.id = json_extract(ev.payload, '$.song_id')
         WHERE ev.type = 'show.current_song' AND json_extract(ev.payload, '$.show_id') = ?
         ORDER BY ev.seq",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let handoffs_done: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM handoff_done WHERE show_id = ?")
            .bind(id)
            .fetch_one(pool)
            .await?;
    Ok(Json(Report {
        show: plan.show,
        generated_at: now(),
        alerts,
        swaps,
        battery_checks,
        faulty_items,
        song_changes,
        handoffs_done,
    }))
}
