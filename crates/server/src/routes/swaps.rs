//! Swaps and everything else that changes a running show on the v3.1 model:
//! - substitutions (swap / pull / reassign / guest / TX swap) and their steps;
//! - undo of a handoff or pass (10 s, R8);
//! - battery checks per device with the per-show evidence window (R5);
//! - plot and pass edits from any screen (R7);
//! - devices, manual holder fixes, count-in close.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use rfops_core::plan::{
    battery_evidence_start, Holder, Holding, Id, SubKind, SubState, Substitution,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    plan_store::{self, kind_str},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/shows/{id}/devices/{dev}/candidates", get(candidates))
        .route("/api/shows/{id}/swaps", get(list_swaps).post(create_swap))
        .route("/api/swaps/{sid}/in-hand", post(in_hand))
        .route("/api/swaps/{sid}/foh-done", post(foh_done))
        .route("/api/swaps/{sid}/reverse", post(reverse))
        .route("/api/swaps/{sid}/return", post(start_return))
        .route("/api/swaps/{sid}/return-in-hand", post(return_in_hand))
        .route("/api/shows/{id}/undo", post(undo))
        .route("/api/shows/{id}/batteries", get(batteries))
        .route("/api/devices/{id}/battery", post(battery_check))
        .route("/api/shows/{id}/plot", put(set_cell).get(get_plot))
        .route("/api/shows/{id}/passes", post(add_pass))
        .route("/api/passes/{pid}", delete(delete_pass))
        .route("/api/devices/new", post(create_device))
        .route("/api/devices/{id}", patch(update_device))
        .route("/api/devices/{id}/holder", post(set_holder))
        .route("/api/shows/{id}/count-in/close", post(close_count_in))
}

type SubRow = (
    Id,
    Id,
    String,
    Option<Id>,
    Option<Id>,
    Option<Id>,
    Option<Id>,
    Option<Id>,
    bool,
    String,
    String,
    i64,
    Option<Id>,
    Option<i64>,
);

async fn load_sub(s: &AppState, sid: Id) -> ApiResult<(Id, Substitution)> {
    let r: SubRow = sqlx::query_as(
        "SELECT id, show_id, kind, performer_id, failed_id, replacement_id, failed_base_id, replacement_base_id,
                retune, range, state, from_visit, starts_at_song_id, ended_at_visit
         FROM substitutions WHERE id = ?",
    )
    .bind(sid)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok((
        r.1,
        Substitution {
            id: r.0,
            kind: plan_store::sub_kind(&r.2),
            performer: r.3,
            failed: r.4,
            replacement: r.5,
            failed_base: r.6,
            replacement_base: r.7,
            retune: r.8,
            range: plan_store::sub_range(&r.9),
            state: plan_store::sub_state(&r.10),
            from_visit: r.11 as u32,
            starts_at_song: r.12,
            ended_at_visit: r.13.map(|v| v as u32),
        },
    ))
}

fn state_str(s: SubState) -> &'static str {
    match s {
        SubState::Planned => "planned",
        SubState::Recorded => "recorded",
        SubState::InHand => "in_hand",
        SubState::FohConfirmed => "foh_confirmed",
        SubState::Reversed => "reversed",
        SubState::ReturnRecorded => "return_recorded",
        SubState::Returned => "returned",
    }
}

async fn set_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    sid: Id,
    st: SubState,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE substitutions SET state = ? WHERE id = ?")
        .bind(state_str(st))
        .bind(sid)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- candidates and swaps

async fn candidates(
    State(s): State<AppState>,
    Path((id, dev)): Path<(Id, Id)>,
) -> ApiResult<Json<Value>> {
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let list = st.swap_candidates(dev);
    let holder = st.holding(dev).holder.person();
    let planned = st
        .current_visit()
        .and_then(|c| st.planned(dev, c.song, c.no, None));
    Ok(Json(json!({
        "failed": dev,
        "performer": holder.or(planned),
        "candidates": list.iter().map(|c| json!({
            "device": c.device,
            "label": names.devices.get(&c.device),
            "tier": c.tier,
            "retune": c.retune,
            "flag": c.flag,
        })).collect::<Vec<_>>(),
        "names": names,
    })))
}

#[derive(Deserialize)]
pub struct NewSwap {
    #[serde(default = "default_kind")]
    kind: String,
    failed: Option<Id>,
    replacement: Option<Id>,
    /// Defaults to whoever has (or is planned on) the failed device now.
    performer: Option<Id>,
    /// this_song | until_fixed (default, R1) | rest_of_show
    #[serde(default = "default_range")]
    range: String,
    /// IEM: sync the spare to the failed pack's TX (R6). Defaults to true when TXs differ.
    retune: Option<bool>,
    failed_base: Option<Id>,
    replacement_base: Option<Id>,
    /// A swap for a later song (Extend / pick ahead): no steps, normal handoffs then.
    starts_at_song: Option<Id>,
    #[serde(default)]
    component: Option<String>,
}
fn default_kind() -> String {
    "swap".into()
}
fn default_range() -> String {
    "until_fixed".into()
}

async fn create_swap(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<NewSwap>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let kind = plan_store::sub_kind(&b.kind);
    if !["swap", "pull", "reassign", "guest", "tx_swap"].contains(&b.kind.as_str()) {
        return Err(ApiError::bad("kind must be swap, pull, reassign, guest or tx_swap"));
    }
    if !["this_song", "until_fixed", "rest_of_show"].contains(&b.range.as_str()) {
        return Err(ApiError::bad("range must be this_song, until_fixed or rest_of_show"));
    }
    let cur = st.current_visit().cloned();
    let visit = cur.as_ref().map(|c| c.no).unwrap_or(0);
    let failed_dev = b.failed.and_then(|f| st.device(f).cloned());
    if kind != SubKind::TxSwap && failed_dev.is_none() {
        return Err(ApiError::bad("which device failed?"));
    }
    if matches!(kind, SubKind::Swap | SubKind::Reassign | SubKind::Guest) {
        let r = b.replacement.and_then(|r| st.device(r)).ok_or_else(|| ApiError::bad("pick the replacement"))?;
        if Some(r.kind) != failed_dev.as_ref().map(|f| f.kind) {
            return Err(ApiError::bad("the replacement must be the same kind (mic, IEM pack, guitar pack)"));
        }
    }
    if kind == SubKind::TxSwap && (b.failed_base.is_none() || b.replacement_base.is_none()) {
        return Err(ApiError::bad("a TX swap needs the failed and the replacement transmitter"));
    }
    let performer = b.performer.or_else(|| {
        let f = b.failed?;
        st.holding(f).holder.person().or_else(|| {
            let c = cur.as_ref()?;
            st.planned(f, c.song, c.no, None)
        })
    });
    if matches!(kind, SubKind::Swap | SubKind::Reassign | SubKind::Guest | SubKind::Pull)
        && performer.is_none()
        && b.range != "rest_of_show"
    {
        return Err(ApiError::bad("nobody has that device now; choose whose device to swap"));
    }
    let retune = b.retune.unwrap_or_else(|| {
        kind == SubKind::Swap
            && failed_dev.as_ref().is_some_and(|f| f.kind == rfops_core::plan::Kind::Iem)
            && b.replacement.and_then(|r| st.device(r)).map(|r| r.base) != failed_dev.as_ref().map(|f| f.base)
    });
    // A swap for a later song has no steps (B-4); everything else starts Recorded.
    let future = b.starts_at_song.is_some_and(|sg| {
        let cur_pos = cur.as_ref().and_then(|c| st.position(c.song));
        st.position(sg).zip(cur_pos).is_none_or(|(a, c)| a > c)
    });
    let state = if future { SubState::Planned } else { SubState::Recorded };
    let mut tx = s.pool.begin().await?;
    let sid: Id = sqlx::query_scalar(
        "INSERT INTO substitutions (show_id, kind, performer_id, failed_id, replacement_id, failed_base_id,
           replacement_base_id, retune, range, state, from_visit, starts_at_song_id, failed_component, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(id)
    .bind(&b.kind)
    .bind(if b.range == "rest_of_show" && b.performer.is_none() { None } else { performer })
    .bind(b.failed)
    .bind(b.replacement)
    .bind(b.failed_base)
    .bind(b.replacement_base)
    .bind(retune)
    .bind(&b.range)
    .bind(state_str(state))
    .bind(visit)
    .bind(b.starts_at_song)
    .bind(b.component.clone().unwrap_or_else(|| "unknown".into()))
    .bind(&actor.name)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    // A swap or pull says the failed device is suspect (out of service is a separate choice).
    if matches!(kind, SubKind::Swap | SubKind::Pull) {
        if let Some(f) = b.failed {
            sqlx::query("UPDATE devices SET status = 'suspect' WHERE id = ? AND status = 'ok'")
                .bind(f)
                .execute(&mut *tx)
                .await?;
        }
    }
    log_event(
        &mut tx,
        &actor,
        "swap.recorded",
        &json!({
            "show_id": id, "swap_id": sid, "kind": b.kind, "range": b.range, "retune": retune,
            "performer": performer.and_then(|p| names.people.get(&p)),
            "failed": b.failed.and_then(|f| names.devices.get(&f)),
            "replacement": b.replacement.and_then(|r| names.devices.get(&r)),
            "state": state_str(state),
        }),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": sid, "state": state_str(state), "retune": retune }))))
}

/// Swaps of a show with labels (swap cards on backstage and FOH).
async fn list_swaps(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let ch: Vec<(Id, Option<String>)> = sqlx::query_as("SELECT id, console_channel FROM devices")
        .fetch_all(&s.pool)
        .await?;
    let ch: BTreeMap<Id, Option<String>> = ch.into_iter().collect();
    let meta: Vec<(Id, String, String, String)> = sqlx::query_as(
        "SELECT id, created_by, created_at, failed_component FROM substitutions WHERE show_id = ?",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    let meta: BTreeMap<Id, (String, String, String)> =
        meta.into_iter().map(|(i, a, b, c)| (i, (a, b, c))).collect();
    let cur = st.current_visit().cloned();
    let list: Vec<Value> = st
        .subs
        .iter()
        .rev()
        .map(|x| {
            let label = |d: Option<Id>| d.and_then(|d| names.devices.get(&d)).cloned();
            let chan = |d: Option<Id>| d.and_then(|d| ch.get(&d).cloned().flatten());
            let mix = |d: Option<Id>| {
                let (d, c) = (d?, cur.as_ref()?);
                st.mix(d, c.song, c.no)
            };
            let m = meta.get(&x.id);
            json!({
                "id": x.id,
                "kind": x.kind,
                "range": x.range,
                "state": x.state,
                "retune": x.retune,
                "performer": x.performer.and_then(|p| names.people.get(&p)),
                "failed": label(x.failed),
                "failed_id": x.failed,
                "replacement": label(x.replacement),
                "replacement_id": x.replacement,
                "failed_channel": chan(x.failed),
                "replacement_channel": chan(x.replacement),
                "mix": mix(x.replacement),
                "failed_base": x.failed_base.and_then(|b| names.equipment.get(&b)),
                "replacement_base": x.replacement_base.and_then(|b| names.equipment.get(&b)),
                "created_by": m.map(|m| &m.0),
                "created_at": m.map(|m| &m.1),
                "component": m.map(|m| &m.2),
                "active": cur.as_ref().is_some_and(|c| st.sub_applies(x, c.song, c.no)) || x.state == SubState::Planned,
            })
        })
        .collect();
    Ok(Json(json!(list)))
}

async fn in_hand(State(s): State<AppState>, actor: Actor, Path(sid): Path<Id>) -> ApiResult<Json<Value>> {
    let (show, sub) = load_sub(&s, sid).await?;
    if sub.state != SubState::Recorded {
        return Err(ApiError::Conflict("That swap isn't waiting for In hand.".into()));
    }
    let (st, _) = plan_store::load(&s.pool, show).await?;
    let mut tx = s.pool.begin().await?;
    match (sub.kind, sub.performer) {
        (SubKind::TxSwap, _) => {} // packs are retuned one by one from the handoff list
        (_, Some(p)) => {
            let moves = st.sub_in_hand(&sub, p);
            plan_store::write_holdings(&mut tx, &moves, &actor.name).await?;
        }
        _ => {}
    }
    set_state(&mut tx, sid, SubState::InHand).await?;
    log_event(&mut tx, &actor, "swap.in_hand", &json!({ "show_id": show, "swap_id": sid })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": sid, "state": "in_hand" })))
}

async fn foh_done(State(s): State<AppState>, actor: Actor, Path(sid): Path<Id>) -> ApiResult<Json<Value>> {
    let (show, sub) = load_sub(&s, sid).await?;
    if sub.state != SubState::InHand {
        return Err(ApiError::Conflict(
            "Wait for backstage to tap In hand (the spare isn't on the performer yet).".into(),
        ));
    }
    let mut tx = s.pool.begin().await?;
    set_state(&mut tx, sid, SubState::FohConfirmed).await?;
    log_event(&mut tx, &actor, "swap.foh_confirmed", &json!({ "show_id": show, "swap_id": sid })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": sid })))
}

/// Undo a swap: before In hand it simply disappears; after, it is reversed (devices swap back).
async fn reverse(State(s): State<AppState>, actor: Actor, Path(sid): Path<Id>) -> ApiResult<Json<Value>> {
    let (show, sub) = load_sub(&s, sid).await?;
    let (st, _) = plan_store::load(&s.pool, show).await?;
    let mut tx = s.pool.begin().await?;
    match sub.state {
        SubState::Recorded | SubState::Planned => {
            sqlx::query("DELETE FROM substitutions WHERE id = ?").bind(sid).execute(&mut *tx).await?;
        }
        SubState::InHand | SubState::FohConfirmed => {
            if let Some(p) = sub.performer {
                let moves = st.return_in_hand(&sub, p);
                plan_store::write_holdings(&mut tx, &moves, &actor.name).await?;
            }
            set_state(&mut tx, sid, SubState::Reversed).await?;
        }
        _ => return Err(ApiError::Conflict("That swap has already ended.".into())),
    }
    if let Some(f) = sub.failed {
        sqlx::query("UPDATE devices SET status = 'ok' WHERE id = ? AND status = 'suspect'")
            .bind(f)
            .execute(&mut *tx)
            .await?;
    }
    log_event(&mut tx, &actor, "swap.reversed", &json!({ "show_id": show, "swap_id": sid })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": sid })))
}

/// "Return Helen to Red": the original is fixed. Two steps like the swap.
async fn start_return(State(s): State<AppState>, actor: Actor, Path(sid): Path<Id>) -> ApiResult<Json<Value>> {
    let (show, sub) = load_sub(&s, sid).await?;
    if !matches!(sub.state, SubState::InHand | SubState::FohConfirmed) {
        return Err(ApiError::Conflict("Only a swap in use can be returned.".into()));
    }
    let mut tx = s.pool.begin().await?;
    set_state(&mut tx, sid, SubState::ReturnRecorded).await?;
    if let Some(f) = sub.failed {
        sqlx::query("UPDATE devices SET status = 'ok' WHERE id = ?").bind(f).execute(&mut *tx).await?;
    }
    log_event(&mut tx, &actor, "swap.return_recorded", &json!({ "show_id": show, "swap_id": sid })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": sid, "state": "return_recorded" })))
}

async fn return_in_hand(State(s): State<AppState>, actor: Actor, Path(sid): Path<Id>) -> ApiResult<Json<Value>> {
    let (show, sub) = load_sub(&s, sid).await?;
    if sub.state != SubState::ReturnRecorded {
        return Err(ApiError::Conflict("That return isn't waiting for In hand.".into()));
    }
    let (st, _) = plan_store::load(&s.pool, show).await?;
    let visit = st.current_visit().map(|v| v.no).unwrap_or(0);
    let mut tx = s.pool.begin().await?;
    if let Some(p) = sub.performer {
        let moves = st.return_in_hand(&sub, p);
        plan_store::write_holdings(&mut tx, &moves, &actor.name).await?;
    }
    sqlx::query("UPDATE substitutions SET state = 'returned', ended_at_visit = ? WHERE id = ?")
        .bind(visit)
        .bind(sid)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "swap.returned", &json!({ "show_id": show, "swap_id": sid })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": sid, "state": "returned" })))
}

// ---------------------------------------------------------------- undo (R8)

#[derive(Deserialize)]
pub struct UndoBody {
    seq: i64,
}

/// Undo a handoff or pass Done within 10 s, if nothing has touched that device since.
async fn undo(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<UndoBody>,
) -> ApiResult<Json<Value>> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT type, payload, at FROM event_log WHERE seq = ?")
            .bind(b.seq)
            .fetch_optional(&s.pool)
            .await?;
    let (kind, payload, at) = row.ok_or(ApiError::NotFound)?;
    let p: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
    let at = DateTime::parse_from_rfc3339(&at).map(|t| t.with_timezone(&Utc)).ok();
    if at.is_none_or(|t| (Utc::now() - t).num_seconds() > 10) {
        return Err(ApiError::Conflict("Too late to undo (10 seconds).".into()));
    }
    let device = match kind.as_str() {
        "holding.changed" => p["device_id"].as_i64(),
        "pass.done" => p["card"]["device"].as_i64(),
        _ => None,
    }
    .ok_or_else(|| ApiError::bad("only a handoff or pass can be undone here"))?;
    // Refuse if a later event touched the same device (multi-user safety).
    let later: Vec<String> = sqlx::query_scalar(
        "SELECT payload FROM event_log WHERE seq > ? AND type IN ('holding.changed', 'pass.done', 'swap.in_hand', 'swap.returned')",
    )
    .bind(b.seq)
    .fetch_all(&s.pool)
    .await?;
    if later.iter().any(|x| {
        let v: Value = serde_json::from_str(x).unwrap_or(Value::Null);
        v["device_id"].as_i64() == Some(device) || v["card"]["device"].as_i64() == Some(device)
    }) {
        return Err(ApiError::Conflict("Someone changed that device since; can't undo.".into()));
    }
    let mut tx = s.pool.begin().await?;
    if kind == "holding.changed" {
        let before: Holding = serde_json::from_value(p["before"].clone()).unwrap_or_default();
        plan_store::write_holdings(&mut tx, &vec![(device, before)], &actor.name).await?;
    } else {
        let pass = p["pass_id"].as_i64().unwrap_or(0);
        let from = p["card"]["from"].as_i64();
        let (st, _) = plan_store::load(&s.pool, id).await?;
        let mut h = st.holding(device);
        h.holder = from.map(Holder::Person).unwrap_or(Holder::Stock);
        plan_store::write_holdings(&mut tx, &vec![(device, h)], &actor.name).await?;
        sqlx::query("DELETE FROM pass_marks WHERE pass_id = ? AND visit_no = (SELECT MAX(visit_no) FROM pass_marks WHERE pass_id = ?)")
            .bind(pass)
            .bind(pass)
            .execute(&mut *tx)
            .await?;
    }
    log_event(&mut tx, &actor, "undo", &json!({ "show_id": id, "undone_seq": b.seq, "device_id": device })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "undone": b.seq })))
}

// ---------------------------------------------------------------- batteries (R5)

/// When battery checks start counting for this show (UTC).
pub(crate) async fn evidence_start(s: &AppState, show: Id) -> ApiResult<DateTime<Utc>> {
    type ShowTimes = (Option<String>, Option<String>, Option<String>, Option<Id>, Option<String>);
    let row: ShowTimes = sqlx::query_as(
        "SELECT s.day, s.call_time, s.activated_at, s.follows_show_id, e.timezone
         FROM shows s LEFT JOIN events e ON e.id = s.event_id WHERE s.id = ?",
    )
    .bind(show)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    let tz: chrono_tz_lite::Tz = row.4.as_deref().and_then(chrono_tz_lite::parse).unwrap_or_default();
    let call = row.0.as_deref().zip(row.1.as_deref()).and_then(|(d, t)| {
        let d = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?;
        let t = NaiveTime::parse_from_str(t, "%H:%M").ok()?;
        Some(tz.to_utc(d.and_time(t)))
    });
    let activated = row.2.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc));
    let prev_end: Option<DateTime<Utc>> = match row.3 {
        Some(p) => sqlx::query_scalar::<_, Option<String>>("SELECT ended_at FROM shows WHERE id = ?")
            .bind(p)
            .fetch_optional(&s.pool)
            .await?
            .flatten()
            .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
            .map(|t| t.with_timezone(&Utc)),
        None => None,
    };
    // Without a call time: checks from 6 h before the show started count (they're done before
    // show mode), or the last 12 h if it hasn't started. The end of the previous linked show
    // still separates afternoon checks from evening ones (R5).
    let fallback = activated.map(|a| a - chrono::Duration::hours(6));
    Ok(battery_evidence_start(call, fallback, prev_end).unwrap_or_else(|| Utc::now() - chrono::Duration::hours(12)))
}

/// Latest valid check per device for this show.
async fn batteries(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let since = evidence_start(&s, id).await?;
    let rows: Vec<(Id, String, String, String)> = sqlx::query_as(
        "SELECT device_id, result, checked_at, checked_by FROM device_battery_checks
         WHERE checked_at >= ? ORDER BY checked_at",
    )
    .bind(since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    .fetch_all(&s.pool)
    .await?;
    let mut latest: BTreeMap<Id, Value> = BTreeMap::new();
    for (d, r, at, by) in rows {
        latest.insert(d, json!({ "result": r, "at": at, "by": by }));
    }
    Ok(Json(json!({ "since": since.to_rfc3339(), "devices": latest })))
}

#[derive(Deserialize)]
pub struct BatteryBody {
    /// ok | fresh | low
    result: String,
}

async fn battery_check(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<BatteryBody>,
) -> ApiResult<StatusCode> {
    if !["ok", "fresh", "low"].contains(&b.result.as_str()) {
        return Err(ApiError::bad("result must be ok, fresh or low"));
    }
    let exists: Option<Id> = sqlx::query_scalar("SELECT id FROM devices WHERE id = ?")
        .bind(id)
        .fetch_optional(&s.pool)
        .await?;
    exists.ok_or(ApiError::NotFound)?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO device_battery_checks (device_id, result, checked_at, checked_by) VALUES (?, ?, ?, ?)")
        .bind(id)
        .bind(&b.result)
        .bind(now())
        .bind(&actor.name)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "battery.checked", &json!({ "device_id": id, "result": b.result })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- plot edits (R7)

#[derive(Deserialize)]
pub struct CellBody {
    device: Id,
    /// None = the Default column.
    song: Option<Id>,
    /// None = nobody.
    performer: Option<Id>,
    /// For a song cell: remove the override so the Default applies again.
    #[serde(default)]
    clear: bool,
}

async fn set_cell(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<CellBody>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let key = b.song.unwrap_or(0);
    if b.clear {
        sqlx::query("DELETE FROM plot_cells WHERE show_id = ? AND device_id = ? AND song_key = ?")
            .bind(id).bind(b.device).bind(key).execute(&mut *tx).await?;
    } else {
        sqlx::query(
            "INSERT INTO plot_cells (show_id, device_id, song_key, performer_id) VALUES (?, ?, ?, ?)
             ON CONFLICT(show_id, device_id, song_key) DO UPDATE SET performer_id = excluded.performer_id",
        )
        .bind(id).bind(b.device).bind(key).bind(b.performer)
        .execute(&mut *tx).await?;
    }
    log_event(&mut tx, &actor, "plot.changed", &json!({ "show_id": id, "device_id": b.device, "song_id": b.song, "performer_id": b.performer, "clear": b.clear })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The raw plot for editing: explicit cells (song None = Default) and passes.
async fn get_plot(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let cells: Vec<(Id, i64, Option<Id>)> =
        sqlx::query_as("SELECT device_id, song_key, performer_id FROM plot_cells WHERE show_id = ?")
            .bind(id)
            .fetch_all(&s.pool)
            .await?;
    let passes: Vec<(Id, Id, Id, i64, Option<Id>)> = sqlx::query_as(
        "SELECT id, device_id, song_id, seq, to_performer_id FROM passes WHERE show_id = ? ORDER BY song_id, device_id, seq",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!({
        "cells": cells.iter().map(|(d, k, p)| json!({ "device": d, "song": if *k == 0 { None } else { Some(*k) }, "performer": p })).collect::<Vec<_>>(),
        "passes": passes.iter().map(|(i, d, sg, q, to)| json!({ "id": i, "device": d, "song": sg, "seq": q, "to": to })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct PassBody {
    device: Id,
    song: Id,
    to: Option<Id>,
}

async fn add_pass(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<PassBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let mut tx = s.pool.begin().await?;
    let seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) + 1 FROM passes WHERE device_id = ? AND song_id = ?")
        .bind(b.device).bind(b.song).fetch_one(&mut *tx).await?;
    let pid: Id = sqlx::query_scalar(
        "INSERT INTO passes (show_id, device_id, song_id, seq, to_performer_id) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(id).bind(b.device).bind(b.song).bind(seq).bind(b.to)
    .fetch_one(&mut *tx).await?;
    log_event(&mut tx, &actor, "pass.added", &json!({ "show_id": id, "pass_id": pid })).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": pid }))))
}

async fn delete_pass(State(s): State<AppState>, actor: Actor, Path(pid): Path<Id>) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("DELETE FROM passes WHERE id = ?").bind(pid).execute(&mut *tx).await?.rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    log_event(&mut tx, &actor, "pass.deleted", &json!({ "pass_id": pid })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- devices, holders, count-in

#[derive(Deserialize)]
pub struct DeviceBody {
    label: Option<String>,
    kind: Option<String>,
    channel: Option<String>,
    /// Inventory label of the TX / receiver channel.
    base: Option<String>,
    spare: Option<bool>,
    board_order: Option<i64>,
    active: Option<bool>,
}

async fn create_device(State(s): State<AppState>, actor: Actor, Json(b): Json<DeviceBody>) -> ApiResult<(StatusCode, Json<Value>)> {
    let label = b.label.as_deref().map(str::trim).filter(|l| !l.is_empty()).ok_or_else(|| ApiError::bad("label is required"))?;
    let kind = b.kind.as_deref().unwrap_or("mic");
    if !["mic", "iem", "guitar"].contains(&kind) {
        return Err(ApiError::bad("kind must be mic, iem or guitar"));
    }
    let mut tx = s.pool.begin().await?;
    let id: Id = sqlx::query_scalar(
        "INSERT INTO devices (label, kind, console_channel, is_spare, base_id, carrier_id, board_order)
         VALUES (?, ?, ?, ?, (SELECT id FROM equipment WHERE label = ?), (SELECT id FROM equipment WHERE label = ?), COALESCE(?, 1000))
         RETURNING id",
    )
    .bind(label).bind(kind).bind(&b.channel).bind(b.spare.unwrap_or(false)).bind(&b.base).bind(label).bind(b.board_order)
    .fetch_one(&mut *tx).await?;
    log_event(&mut tx, &actor, "device.created", &json!({ "device_id": id, "label": label, "kind": kind })).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn update_device(State(s): State<AppState>, actor: Actor, Path(id): Path<Id>, Json(b): Json<DeviceBody>) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query(
        "UPDATE devices SET label = COALESCE(?, label), kind = COALESCE(?, kind), console_channel = COALESCE(?, console_channel),
           is_spare = COALESCE(?, is_spare), board_order = COALESCE(?, board_order), active = COALESCE(?, active),
           base_id = CASE WHEN ? IS NULL THEN base_id ELSE (SELECT id FROM equipment WHERE label = ?) END
         WHERE id = ?",
    )
    .bind(&b.label).bind(b.kind.as_deref().map(|k| kind_str(plan_store::kind_from(k)))).bind(&b.channel)
    .bind(b.spare).bind(b.board_order).bind(b.active).bind(&b.base).bind(&b.base).bind(id)
    .execute(&mut *tx).await?.rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    log_event(&mut tx, &actor, "device.updated", &json!({ "device_id": id })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct HolderBody {
    /// None = back in stock.
    performer: Option<Id>,
}

/// Manual fix: "RF Ops thinks Helen has it, but it's actually in the case."
async fn set_holder(State(s): State<AppState>, actor: Actor, Path(id): Path<Id>, Json(b): Json<HolderBody>) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let before: Option<(String, Option<Id>)> = sqlx::query_as("SELECT holder_kind, performer_id FROM holdings WHERE device_id = ?")
        .bind(id).fetch_optional(&mut *tx).await?;
    let h = Holding { holder: b.performer.map(Holder::Person).unwrap_or(Holder::Stock), ..Default::default() };
    plan_store::write_holdings(&mut tx, &vec![(id, h)], &actor.name).await?;
    log_event(&mut tx, &actor, "holding.manual", &json!({ "device_id": id, "performer_id": b.performer, "before": before })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// End of show: anything still out (and not kept for the next show) is marked missing.
async fn close_count_in(State(s): State<AppState>, actor: Actor, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let out = st.count_in();
    let mut tx = s.pool.begin().await?;
    for (d, _) in &out {
        sqlx::query("UPDATE devices SET status = 'missing' WHERE id = ?").bind(d).execute(&mut *tx).await?;
    }
    let list: Vec<Value> = out
        .iter()
        .map(|(d, h)| json!({ "device": names.devices.get(d), "last_holder": h.person().and_then(|p| names.people.get(&p)) }))
        .collect();
    log_event(&mut tx, &actor, "count_in.closed", &json!({ "show_id": id, "missing": list })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "missing": list })))
}

/// Minimal fixed-offset time zone support for the event zone (US zones with DST rules), so
/// "14:00 on 2026-06-19 in America/Chicago" becomes a UTC instant without a tz database.
mod chrono_tz_lite {
    use chrono::{Datelike, NaiveDate, NaiveDateTime, TimeZone, Utc};

    #[derive(Clone, Copy, Default)]
    pub struct Tz {
        /// Standard offset in hours (e.g. −6 for Central).
        std: i32,
        dst: bool,
    }

    pub fn parse(name: &str) -> Option<Tz> {
        let (std, dst) = match name {
            "America/New_York" | "America/Detroit" | "US/Eastern" => (-5, true),
            "America/Chicago" | "US/Central" => (-6, true),
            "America/Denver" | "US/Mountain" => (-7, true),
            "America/Phoenix" => (-7, false),
            "America/Los_Angeles" | "US/Pacific" => (-8, true),
            "UTC" | "Etc/UTC" => (0, false),
            _ => return None,
        };
        Some(Tz { std, dst })
    }

    fn nth_sunday(y: i32, m: u32, n: u32) -> NaiveDate {
        let first = NaiveDate::from_ymd_opt(y, m, 1).expect("valid month");
        let offset = (7 - first.weekday().num_days_from_sunday()) % 7;
        first + chrono::Duration::days((offset + 7 * (n - 1)) as i64)
    }

    impl Tz {
        /// US rules: DST from the 2nd Sunday of March 02:00 to the 1st Sunday of November 02:00.
        pub fn to_utc(self, local: NaiveDateTime) -> chrono::DateTime<Utc> {
            let mut off = self.std;
            if self.dst {
                let y = local.year();
                let start = nth_sunday(y, 3, 2).and_hms_opt(2, 0, 0).expect("time");
                let end = nth_sunday(y, 11, 1).and_hms_opt(2, 0, 0).expect("time");
                if local >= start && local < end {
                    off += 1;
                }
            }
            Utc.from_utc_datetime(&(local - chrono::Duration::hours(off as i64)))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn chicago_summer_and_winter() {
            let tz = parse("America/Chicago").unwrap();
            let d = |m, dd, h| NaiveDate::from_ymd_opt(2026, m, dd).unwrap().and_hms_opt(h, 0, 0).unwrap();
            assert_eq!(tz.to_utc(d(6, 19, 14)).to_rfc3339(), "2026-06-19T19:00:00+00:00");
            assert_eq!(tz.to_utc(d(12, 5, 14)).to_rfc3339(), "2026-12-05T20:00:00+00:00");
        }
    }
}
