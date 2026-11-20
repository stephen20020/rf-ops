//! Pre-show checklist (interface spec §12.1) and post-show report (§16) on the v3.1 model.

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use rfops_core::plan::{Holder, Id};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    plan_store,
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/shows/{id}/checklist", get(checklist))
        .route("/api/shows/{id}/checklist/tick", post(tick))
        .route("/api/shows/{id}/checklist/custom", post(add_custom))
        .route("/api/checklist/custom/{cid}", delete(delete_custom))
        .route("/api/shows/{id}/post-show", get(report))
}

/// Manual items, in order. Keys are stable (ticks are stored by key).
const MANUAL: &[(&str, &str, &str)] = &[
    ("line_check", "Line check: every mic talked into at FOH", "Ask FOH to confirm each channel moves."),
    ("iem_check", "IEM packs on the right TX, volume set, limiter on", "Each performer hears their own mix."),
    ("wifi", "Wi-Fi on 5 GHz channels 36–64 only", "The GLXD6+ guitar packs use 5.8 GHz above that."),
    ("dlive", "dLive scene loaded; FOH confirms the mic order", "Ch 33–44 = the colour mics in fader order."),
    ("paper", "Paper plot printed for every A2 and FOH", "Use Print paper plot at the top of this page."),
];

async fn ticks(s: &AppState, show: Id) -> ApiResult<BTreeMap<String, (String, String)>> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT item_key, ticked_by, ticked_at FROM checklist_ticks WHERE show_id = ?")
            .bind(show)
            .fetch_all(&s.pool)
            .await?;
    Ok(rows.into_iter().map(|(k, b, a)| (k, (b, a))).collect())
}

fn item(key: &str, label: &str, auto: bool, ok: bool, detail: String, tick: Option<&(String, String)>) -> Value {
    json!({
        "key": key,
        "label": label,
        "auto": auto,
        "ok": ok,
        "detail": detail,
        "ticked_by": tick.map(|t| &t.0),
        "ticked_at": tick.map(|t| &t.1),
    })
}

fn list(names: &[String], max: usize) -> String {
    let mut s = names.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > max {
        s.push_str(&format!(" +{} more", names.len() - max));
    }
    s
}

async fn checklist(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let ticks = ticks(&s, id).await?;
    let label = |d: &Id| names.devices.get(d).cloned().unwrap_or_default();

    // Devices this show uses (anyone planned on them in any song or pass).
    let mut used: BTreeSet<Id> = BTreeSet::new();
    for song in &st.songs {
        let (chains, _) = st.chains(song.id, 1);
        for (d, c) in chains {
            if c.holders.iter().any(Option::is_some) {
                used.insert(d);
            }
        }
    }
    let since = crate::routes::swaps::evidence_start(&s, id).await?;
    let checked: BTreeMap<Id, String> = sqlx::query_as::<_, (Id, String)>(
        "SELECT device_id, result FROM device_battery_checks WHERE checked_at >= ? ORDER BY checked_at",
    )
    .bind(since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    .fetch_all(&s.pool)
    .await?
    .into_iter()
    .collect();

    let mut items = Vec::new();
    // 1. Batteries on everything used today (R5: checks from before this show don't count).
    let missing: Vec<String> = used.iter().filter(|d| !checked.contains_key(d)).map(label).collect();
    let low: Vec<String> = used.iter().filter(|d| checked.get(d).is_some_and(|r| r == "low")).map(label).collect();
    items.push(item(
        "batteries",
        "Battery check on every device used in this show",
        true,
        missing.is_empty() && low.is_empty() && !used.is_empty(),
        if used.is_empty() {
            "No devices are planned yet.".into()
        } else if missing.is_empty() && low.is_empty() {
            format!("All {} checked.", used.len())
        } else {
            let mut d = format!("{}/{} checked.", used.len() - missing.len(), used.len());
            if !missing.is_empty() {
                d.push_str(&format!(" Not checked: {}.", list(&missing, 8)));
            }
            if !low.is_empty() {
                d.push_str(&format!(" LOW: {}.", list(&low, 8)));
            }
            d
        },
        None,
    ));
    // 2. Spares ready.
    let spares: Vec<&rfops_core::plan::Device> = st.devices.iter().filter(|d| d.is_spare).collect();
    let spare_missing: Vec<String> = spares.iter().filter(|d| !checked.contains_key(&d.id)).map(|d| d.label.clone()).collect();
    items.push(item(
        "spares",
        "Spares batteried, tested and staged",
        true,
        spare_missing.is_empty(),
        if spares.is_empty() {
            "No spares marked (fine if you run without a spare; mark them in Admin → Devices).".into()
        } else if spare_missing.is_empty() {
            format!("{} spare(s) checked.", spares.len())
        } else {
            format!("Not checked: {}.", list(&spare_missing, 8))
        },
        None,
    ));
    // 3. The plan has no problems in any song.
    let mut problems = 0usize;
    for song in &st.songs {
        let (chains, conf) = st.chains(song.id, 1);
        problems += conf.len() + st.conflicts_for(song.id, &chains).len();
    }
    items.push(item(
        "plan",
        "No problems in the plan (two packs, broken device planned, absent person…)",
        true,
        problems == 0,
        if problems == 0 { "Plan is clean.".into() } else { format!("{problems} to fix: see Backstage → Check the plan.") },
        None,
    ));
    // 4. Everything for the first song handed out.
    let b = st.board();
    let gives = if b.current_song.is_none() {
        b.coming_up.iter().filter(|h| matches!(h.action, rfops_core::plan::Action::Give { .. })).count()
    } else {
        0
    };
    items.push(item(
        "handed_out",
        "Every device for the first song handed out",
        true,
        gives == 0,
        if gives == 0 { "Done.".into() } else { format!("{gives} still to give out (Backstage).") },
        None,
    ));
    // 5. Playback bridge (only if the show uses MIDI cues).
    let uses_midi: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM songs WHERE show_id = ? AND midi_trigger IS NOT NULL)")
        .bind(id)
        .fetch_one(&s.pool)
        .await?;
    if uses_midi {
        let bridge = s.midi.lock().expect("midi poisoned").bridge.clone();
        let online = bridge.as_ref().is_some_and(|b| b.last_seen.elapsed() < crate::midi::ALERT_AFTER);
        items.push(item(
            "midi",
            "Playback bridge online (send a test cue)",
            true,
            online,
            match bridge {
                Some(b) if online => format!("{} online.", b.name),
                Some(b) => format!("{} last heard {}.", b.name, b.last_seen_at),
                None => "No bridge has connected.".into(),
            },
            None,
        ));
    }
    for (k, l, hint) in MANUAL {
        let t = ticks.get(*k);
        items.push(item(k, l, false, t.is_some(), (*hint).into(), t));
    }
    let custom: Vec<(Id, String)> = sqlx::query_as("SELECT id, label FROM checklist_custom WHERE show_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&s.pool)
        .await?;
    for (cid, l) in custom {
        let key = format!("custom:{cid}");
        let t = ticks.get(&key);
        let mut v = item(&key, &l, false, t.is_some(), String::new(), t);
        v["custom_id"] = json!(cid);
        items.push(v);
    }
    let done = items.iter().filter(|i| i["ok"] == true).count();
    let mode: String = sqlx::query_scalar("SELECT mode FROM shows WHERE id = ?").bind(id).fetch_one(&s.pool).await?;
    Ok(Json(json!({ "items": items, "done": done, "total": items.len(), "mode": mode })))
}

#[derive(Deserialize)]
pub struct TickBody {
    key: String,
    ticked: bool,
}

async fn tick(State(s): State<AppState>, actor: Actor, Path(id): Path<Id>, Json(b): Json<TickBody>) -> ApiResult<StatusCode> {
    let known = MANUAL.iter().any(|(k, _, _)| *k == b.key) || b.key.starts_with("custom:");
    if !known {
        return Err(ApiError::bad("that item ticks itself"));
    }
    let mut tx = s.pool.begin().await?;
    if b.ticked {
        sqlx::query("INSERT OR REPLACE INTO checklist_ticks (show_id, item_key, ticked_by, ticked_at) VALUES (?, ?, ?, ?)")
            .bind(id).bind(&b.key).bind(&actor.name).bind(now()).execute(&mut *tx).await?;
    } else {
        sqlx::query("DELETE FROM checklist_ticks WHERE show_id = ? AND item_key = ?")
            .bind(id).bind(&b.key).execute(&mut *tx).await?;
    }
    log_event(&mut tx, &actor, "checklist.ticked", &json!({ "show_id": id, "key": b.key, "ticked": b.ticked })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CustomBody {
    label: String,
}

async fn add_custom(State(s): State<AppState>, actor: Actor, Path(id): Path<Id>, Json(b): Json<CustomBody>) -> ApiResult<StatusCode> {
    let l = b.label.trim();
    if l.is_empty() {
        return Err(ApiError::bad("write the item"));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO checklist_custom (show_id, label) VALUES (?, ?)").bind(id).bind(l).execute(&mut *tx).await?;
    log_event(&mut tx, &actor, "checklist.item_added", &json!({ "show_id": id, "label": l })).await?;
    tx.commit().await?;
    Ok(StatusCode::CREATED)
}

async fn delete_custom(State(s): State<AppState>, actor: Actor, Path(cid): Path<Id>) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    sqlx::query("DELETE FROM checklist_custom WHERE id = ?").bind(cid).execute(&mut *tx).await?;
    log_event(&mut tx, &actor, "checklist.item_removed", &json!({ "id": cid })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- post-show report

type AlertRow = (Id, String, String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);

async fn report(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    let show: (String, Option<String>, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT name, day, set_name, activated_at, ended_at FROM shows WHERE id = ?")
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or(ApiError::NotFound)?;
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let dev = |d: Option<Id>| d.and_then(|d| names.devices.get(&d).cloned());
    let person = |p: Option<Id>| p.and_then(|p| names.people.get(&p).cloned());

    // Songs played, in order, with how they were reached.
    let visits: Vec<(i64, Id, String, String)> = sqlx::query_as(
        "SELECT visit_no, song_id, started_at, via FROM song_visits WHERE show_id = ? ORDER BY visit_no",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    let played: Vec<Value> = visits
        .iter()
        .map(|(n, sg, at, via)| json!({ "visit": n, "song": names.songs.get(sg), "at": at, "via": via }))
        .collect();
    let played_ids: BTreeSet<Id> = visits.iter().map(|v| v.1).collect();
    let not_played: Vec<&String> = st.songs.iter().filter(|x| !played_ids.contains(&x.id)).filter_map(|x| names.songs.get(&x.id)).collect();

    // Swaps with their timeline from the event log.
    let swap_events: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT type, at, actor, payload FROM event_log WHERE type LIKE 'swap.%' AND json_extract(payload, '$.show_id') = ? ORDER BY seq",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    let mut timeline: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for (t, at, actor, p) in &swap_events {
        let v: Value = serde_json::from_str(p).unwrap_or(Value::Null);
        if let Some(sid) = v["swap_id"].as_i64() {
            timeline.entry(sid).or_default().push(json!({ "step": t.trim_start_matches("swap."), "at": at, "by": actor }));
        }
    }
    let meta: BTreeMap<Id, (String, String)> = sqlx::query_as::<_, (Id, String, String)>(
        "SELECT id, failed_component, created_at FROM substitutions WHERE show_id = ?",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?
    .into_iter()
    .map(|(i, c, a)| (i, (c, a)))
    .collect();
    let swaps: Vec<Value> = st
        .subs
        .iter()
        .map(|x| {
            json!({
                "kind": x.kind, "range": x.range, "state": x.state,
                "performer": person(x.performer), "failed": dev(x.failed), "replacement": dev(x.replacement),
                "component": meta.get(&x.id).map(|m| &m.0), "at": meta.get(&x.id).map(|m| &m.1),
                "timeline": timeline.get(&x.id).cloned().unwrap_or_default(),
            })
        })
        .collect();

    // Problems reported in this show.
    let alerts: Vec<AlertRow> = sqlx::query_as(
        "SELECT id, severity, title, state, opened_at, opened_by, acked_at, acked_by, resolved_at, resolved_by, resolution_note
         FROM alerts WHERE show_id = ? ORDER BY opened_at",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    let problems: Vec<Value> = alerts
        .iter()
        .map(|a| json!({
            "severity": a.1, "title": a.2, "state": a.3, "opened_at": a.4, "opened_by": a.5,
            "acked_at": a.6, "acked_by": a.7, "resolved_at": a.8, "resolved_by": a.9, "note": a.10,
        }))
        .collect();

    // Batteries for this show.
    let since = crate::routes::swaps::evidence_start(&s, id).await?;
    let checks: Vec<(Id, String, String, String)> = sqlx::query_as(
        "SELECT device_id, result, checked_at, checked_by FROM device_battery_checks WHERE checked_at >= ? ORDER BY checked_at",
    )
    .bind(since.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    .fetch_all(&s.pool)
    .await?;
    let mut by_result: BTreeMap<String, usize> = BTreeMap::new();
    let mut lows = Vec::new();
    for (d, r, at, by) in &checks {
        *by_result.entry(r.clone()).or_default() += 1;
        if r == "low" {
            lows.push(json!({ "device": names.devices.get(d), "at": at, "by": by }));
        }
    }

    // Devices needing attention after the show.
    let attention: Vec<(Id, String, String)> =
        sqlx::query_as("SELECT id, label, status FROM devices WHERE active = 1 AND status <> 'ok' ORDER BY board_order, id")
            .fetch_all(&s.pool)
            .await?;
    let attention: Vec<Value> = attention
        .iter()
        .map(|(d, l, stt)| {
            let h = st.holding(*d).holder;
            json!({ "device": l, "status": stt, "with": match h { Holder::Person(p) => names.people.get(&p).cloned(), _ => None } })
        })
        .collect();
    let still_out: Vec<Value> = st
        .count_in()
        .iter()
        .map(|(d, h)| json!({ "device": names.devices.get(d), "with": h.person().and_then(|p| names.people.get(&p)) }))
        .collect();

    // Plan changes made during the show, and handoffs left undone.
    let changes: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT type, at, actor, payload FROM event_log
         WHERE type IN ('plot.changed', 'pass.added', 'pass.deleted', 'show.imported', 'device.status', 'holding.manual')
           AND at >= COALESCE(?, '9999') ORDER BY seq",
    )
    .bind(&show.3)
    .fetch_all(&s.pool)
    .await?;
    let changes: Vec<Value> = changes
        .iter()
        .map(|(t, at, a, p)| json!({ "type": t, "at": at, "by": a, "detail": serde_json::from_str::<Value>(p).unwrap_or(Value::Null) }))
        .collect();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM handoff_left WHERE show_id = ?").bind(id).fetch_one(&s.pool).await?;
    let handoffs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM event_log WHERE type IN ('holding.changed', 'pass.done') AND json_extract(payload, '$.show_id') = ?",
    )
    .bind(id)
    .fetch_one(&s.pool)
    .await?;

    Ok(Json(json!({
        "show": { "name": show.0, "day": show.1, "set": show.2, "started_at": show.3, "ended_at": show.4 },
        "played": played,
        "not_played": not_played,
        "swaps": swaps,
        "problems": problems,
        "batteries": { "since": since.to_rfc3339(), "counts": by_result, "low": lows },
        "attention": attention,
        "still_out": still_out,
        "changes": changes,
        "handoffs_done": handoffs,
        "handoffs_left": left,
    })))
}
