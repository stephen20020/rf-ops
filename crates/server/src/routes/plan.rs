//! The board (core rules applied to a show), handoff / pass Done, and show files.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rfops_core::{
    plan::{Action, Id},
    showfile::{self, Inventory, Level, ShowFile},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    db::{log_event, Actor},
    error::{ApiError, ApiResult},
    plan_store::{self, kind_from, kind_str},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/shows/{id}/board", get(board))
        .route("/api/shows/{id}/moves/done", post(handoff_done))
        .route("/api/shows/{id}/moves/leave", post(handoff_leave))
        .route("/api/shows/{id}/passes/{pass}/done", post(pass_done))
        .route("/api/devices/{id}/status", post(device_status))
        .route("/api/devices", get(list_devices))
        .route("/api/showfile/check", post(showfile_check))
        .route("/api/showfile/import", post(showfile_import))
        .route("/api/shows/{id}/showfile", get(showfile_export))
}

async fn ensure_show(s: &AppState, id: Id) -> ApiResult<()> {
    let ok: Option<i64> = sqlx::query_scalar("SELECT id FROM shows WHERE id = ?")
        .bind(id)
        .fetch_optional(&s.pool)
        .await?;
    ok.map(|_| ()).ok_or(ApiError::NotFound)
}

/// Everything a screen needs: handoffs (overdue / coming up), passes, conflicts, usage, names.
async fn board(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Json<Value>> {
    ensure_show(&s, id).await?;
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let b = st.board();
    let cur = st.current_visit().cloned();
    let devices: Vec<Value> = st
        .devices
        .iter()
        .map(|d| {
            let h = st.holding(d.id);
            json!({
                "id": d.id,
                "label": d.label,
                "kind": d.kind,
                "is_spare": d.is_spare,
                "usage": st.usage(d.id),
                "holder": h.holder,
                "mix": cur.as_ref().and_then(|c| st.mix(d.id, c.song, c.no)),
            })
        })
        .collect();
    Ok(Json(json!({
        "board": b,
        "devices": devices,
        "count_in": st.count_in(),
        "names": names,
    })))
}

#[derive(Deserialize)]
pub struct DoneBody {
    device: Id,
    /// The action exactly as the board showed it, so a stale tap can't do the wrong thing.
    action: Action,
}

async fn handoff_done(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<DoneBody>,
) -> ApiResult<Json<Value>> {
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let board = st.board();
    let h = board
        .now
        .iter()
        .chain(board.coming_up.iter())
        .find(|h| h.device == b.device && h.action == b.action)
        .cloned()
        .ok_or_else(|| {
            ApiError::Conflict("That handoff isn't needed any more (someone already did it, or the plan changed).".into())
        })?;
    let moves = st.do_handoff(&h);
    let before = st.holding(h.device);
    let mut tx = s.pool.begin().await?;
    plan_store::write_holdings(&mut tx, &moves, &actor.name).await?;
    log_event(
        &mut tx,
        &actor,
        "holding.changed",
        &json!({
            "show_id": id,
            "device_id": h.device,
            "device": names.devices.get(&h.device),
            "action": h.action,
            "before": before,
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "done": h })))
}

#[derive(Deserialize)]
pub struct LeaveBody {
    device: Id,
}

/// "Leave it": skip an overdue handoff for this song visit.
async fn handoff_leave(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<LeaveBody>,
) -> ApiResult<StatusCode> {
    let (st, _) = plan_store::load(&s.pool, id).await?;
    let v = st
        .current_visit()
        .map(|v| v.no)
        .ok_or_else(|| ApiError::bad("the show hasn't started"))?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT OR IGNORE INTO handoff_left (show_id, visit_no, device_id) VALUES (?, ?, ?)")
        .bind(id)
        .bind(v)
        .bind(b.device)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "handoff.left", &json!({ "show_id": id, "device_id": b.device, "visit": v })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pass_done(
    State(s): State<AppState>,
    actor: Actor,
    Path((id, pass)): Path<(Id, Id)>,
) -> ApiResult<Json<Value>> {
    let (st, _) = plan_store::load(&s.pool, id).await?;
    let board = st.board();
    let card = board
        .passes
        .iter()
        .find(|p| p.pass == pass && !p.prep)
        .cloned()
        .ok_or_else(|| ApiError::Conflict("That pass isn't due now.".into()))?;
    let (moves, (pid, visit)) = st.pass_done(&card);
    let mut tx = s.pool.begin().await?;
    plan_store::write_holdings(&mut tx, &moves, &actor.name).await?;
    sqlx::query("INSERT INTO pass_marks (pass_id, visit_no, by_name, at) VALUES (?, ?, ?, ?)")
        .bind(pid)
        .bind(visit)
        .bind(&actor.name)
        .bind(crate::db::now())
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "pass.done", &json!({ "show_id": id, "pass_id": pid, "card": card })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "done": card })))
}

#[derive(Deserialize)]
pub struct StatusBody {
    status: String,
}

/// Mark a device ok / suspect / out / missing.
async fn device_status(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<Id>,
    Json(b): Json<StatusBody>,
) -> ApiResult<StatusCode> {
    if !["ok", "suspect", "out", "missing"].contains(&b.status.as_str()) {
        return Err(ApiError::bad("status must be ok, suspect, out or missing"));
    }
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("UPDATE devices SET status = ? WHERE id = ?")
        .bind(&b.status)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(ApiError::NotFound);
    }
    log_event(&mut tx, &actor, "device.status", &json!({ "device_id": id, "status": b.status })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_devices(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<(Id, String, String, Option<String>, bool, String)> = sqlx::query_as(
        "SELECT id, label, kind, console_channel, is_spare, status FROM devices WHERE active = 1 ORDER BY board_order, id",
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(|(id, label, kind, ch, spare, status)| json!({ "id": id, "label": label, "kind": kind, "channel": ch, "is_spare": spare, "status": status }))
        .collect::<Vec<_>>())))
}

// ---------------------------------------------------------------- show files

async fn inventory(s: &AppState) -> ApiResult<Inventory> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT label, kind FROM devices WHERE active = 1")
        .fetch_all(&s.pool)
        .await?;
    Ok(Inventory {
        devices: rows.into_iter().map(|(l, k)| (l, kind_from(&k))).collect(),
    })
}

fn problems_json(p: &[showfile::Problem]) -> Value {
    json!({
        "ok": !p.iter().any(|x| x.level == Level::Error),
        "errors": p.iter().filter(|x| x.level == Level::Error).collect::<Vec<_>>(),
        "warnings": p.iter().filter(|x| x.level == Level::Warning).collect::<Vec<_>>(),
    })
}

/// Server-side checks the file format can't do alone (TX labels for mixes, `follows`).
async fn server_checks(s: &AppState, f: &ShowFile) -> ApiResult<Vec<showfile::Problem>> {
    let mut out = Vec::new();
    for tx in f.mixes.keys() {
        let found: Option<i64> = sqlx::query_scalar("SELECT id FROM equipment WHERE label = ?")
            .bind(tx)
            .fetch_optional(&s.pool)
            .await?;
        if found.is_none() {
            out.push(showfile::Problem {
                level: Level::Error,
                at: format!("mixes → {tx}"),
                message: format!("There's no transmitter called \"{tx}\" in the inventory."),
            });
        }
    }
    if let Some(prev) = &f.follows {
        let found: Option<i64> = sqlx::query_scalar("SELECT id FROM shows WHERE name = ?")
            .bind(prev)
            .fetch_optional(&s.pool)
            .await?;
        if found.is_none() {
            out.push(showfile::Problem {
                level: Level::Error,
                at: "follows".into(),
                message: format!("There's no show called \"{prev}\" to follow."),
            });
        }
    }
    Ok(out)
}

/// POST a YAML show file: plain-English errors and warnings; nothing is changed.
async fn showfile_check(State(s): State<AppState>, body: String) -> ApiResult<Json<Value>> {
    let f = match showfile::parse(&body) {
        Ok(f) => f,
        Err(p) => return Ok(Json(problems_json(&[p]))),
    };
    let (mut probs, _) = showfile::check(&f, &inventory(&s).await?);
    probs.extend(server_checks(&s, &f).await?);
    Ok(Json(problems_json(&probs)))
}

#[derive(Deserialize)]
pub struct ImportQuery {
    #[serde(default)]
    dry_run: bool,
}

/// POST a YAML show file: creates the show, or updates the one with the same name. Songs are
/// matched by title (so a live show keeps its history); songs no longer in the file are removed
/// unless they were already played.
async fn showfile_import(
    State(s): State<AppState>,
    actor: Actor,
    Query(q): Query<ImportQuery>,
    body: String,
) -> ApiResult<Response> {
    let f = match showfile::parse(&body) {
        Ok(f) => f,
        Err(p) => return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(problems_json(&[p]))).into_response()),
    };
    let (mut probs, compiled) = showfile::check(&f, &inventory(&s).await?);
    probs.extend(server_checks(&s, &f).await?);
    let Some(c) = compiled.filter(|_| !probs.iter().any(|p| p.level == Level::Error)) else {
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(problems_json(&probs))).into_response());
    };
    if q.dry_run {
        return Ok(Json(problems_json(&probs)).into_response());
    }

    // Read before the transaction (a second connection inside it would wait on its lock).
    let channel: u8 = crate::midi::setting(&s.pool, "midi.channel")
        .await?
        .and_then(|c| c.parse().ok())
        .unwrap_or(16);
    let mut tx = s.pool.begin().await?;
    // Event.
    let event_id: Option<Id> = match &f.event {
        Some(name) => Some(
            sqlx::query_scalar(
                "INSERT INTO events (name, timezone) VALUES (?, ?)
                 ON CONFLICT(name) DO UPDATE SET timezone = COALESCE(?, events.timezone) RETURNING id",
            )
            .bind(name)
            .bind(f.timezone.clone().unwrap_or_else(|| "America/Chicago".into()))
            .bind(&f.timezone)
            .fetch_one(&mut *tx)
            .await?,
        ),
        None => None,
    };
    let follows: Option<Id> = match &f.follows {
        Some(n) => sqlx::query_scalar("SELECT id FROM shows WHERE name = ?").bind(n).fetch_optional(&mut *tx).await?,
        None => None,
    };
    // Show (by name).
    let existing: Option<Id> = sqlx::query_scalar("SELECT id FROM shows WHERE name = ?")
        .bind(&f.show)
        .fetch_optional(&mut *tx)
        .await?;
    let call = f.call.clone().or_else(|| {
        f.concert.as_ref().and_then(|c| {
            let (h, m) = c.split_once(':')?;
            let h: i64 = h.parse().ok()?;
            Some(format!("{:02}:{m}", (h + 23) % 24))
        })
    });
    let show_id: Id = match existing {
        Some(id) => {
            sqlx::query("UPDATE shows SET event_id = ?, day = ?, set_name = ?, starts_at = ?, call_time = ?, follows_show_id = ? WHERE id = ?")
                .bind(event_id).bind(&f.day).bind(&f.set_name).bind(&f.concert).bind(&call).bind(follows).bind(id)
                .execute(&mut *tx).await?;
            id
        }
        None => sqlx::query_scalar(
            "INSERT INTO shows (name, event_id, day, set_name, starts_at, call_time, follows_show_id) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&f.show).bind(event_id).bind(&f.day).bind(&f.set_name).bind(&f.concert).bind(&call).bind(follows)
        .fetch_one(&mut *tx)
        .await?,
    };

    // People (by name).
    let mut person: BTreeMap<String, Id> = BTreeMap::new();
    for p in &c.people {
        let id: Id = match sqlx::query_scalar::<_, Id>("SELECT id FROM performers WHERE name = ?")
            .bind(&p.name)
            .fetch_optional(&mut *tx)
            .await?
        {
            Some(id) => {
                if p.zone.is_some() || p.notes.is_some() {
                    sqlx::query("UPDATE performers SET zone = COALESCE(?, zone), notes = COALESCE(?, notes) WHERE id = ?")
                        .bind(&p.zone).bind(&p.notes).bind(id).execute(&mut *tx).await?;
                }
                id
            }
            None => sqlx::query_scalar("INSERT INTO performers (name, zone, notes) VALUES (?, ?, ?) RETURNING id")
                .bind(&p.name).bind(&p.zone).bind(p.notes.clone().unwrap_or_default())
                .fetch_one(&mut *tx).await?,
        };
        person.insert(p.name.clone(), id);
    }
    sqlx::query("DELETE FROM performer_absences WHERE show_id = ?").bind(show_id).execute(&mut *tx).await?;
    for p in c.people.iter().filter(|p| p.absent) {
        sqlx::query("INSERT INTO performer_absences (show_id, performer_id) VALUES (?, ?)")
            .bind(show_id).bind(person[&p.name]).execute(&mut *tx).await?;
    }

    // New devices.
    for d in &c.new_devices {
        sqlx::query("INSERT INTO devices (label, kind, console_channel, is_spare, guest_owner_id) VALUES (?, ?, ?, ?, ?)")
            .bind(&d.label).bind(kind_str(d.kind)).bind(&d.channel).bind(d.spare)
            .bind(d.owner.as_ref().and_then(|o| person.get(o)))
            .execute(&mut *tx).await?;
    }
    let dev_rows: Vec<(String, Id)> = sqlx::query_as("SELECT label, id FROM devices").fetch_all(&mut *tx).await?;
    let device: BTreeMap<String, Id> = dev_rows.into_iter().collect();

    // Songs: matched by title; positions follow the file.
    let old: Vec<(Id, String)> = sqlx::query_as("SELECT id, title FROM songs WHERE show_id = ?")
        .bind(show_id).fetch_all(&mut *tx).await?;
    let mut song_ids = Vec::new();
    for (i, sg) in c.songs.iter().enumerate() {
        let trigger = sg.pc.map(|v| json!({ "type": "program_change", "channel": channel, "value": v }).to_string());
        let id: Id = match old.iter().find(|(_, t)| *t == sg.title) {
            Some((id, _)) => {
                sqlx::query("UPDATE songs SET position = ?, midi_trigger = ?, skipped = ?, notes = ? WHERE id = ?")
                    .bind(i as i64 + 1).bind(&trigger).bind(sg.skip).bind(sg.notes.clone().unwrap_or_default()).bind(id)
                    .execute(&mut *tx).await?;
                *id
            }
            None => sqlx::query_scalar("INSERT INTO songs (show_id, position, title, midi_trigger, skipped, notes) VALUES (?, ?, ?, ?, ?, ?) RETURNING id")
                .bind(show_id).bind(i as i64 + 1).bind(&sg.title).bind(&trigger).bind(sg.skip).bind(sg.notes.clone().unwrap_or_default())
                .fetch_one(&mut *tx).await?,
        };
        song_ids.push(id);
    }
    for (id, title) in old.iter().filter(|(id, _)| !song_ids.contains(id)) {
        let played: Option<i64> = sqlx::query_scalar("SELECT 1 FROM song_visits WHERE song_id = ? LIMIT 1")
            .bind(id).fetch_optional(&mut *tx).await?;
        if played.is_some() {
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(problems_json(&[showfile::Problem {
                    level: Level::Error,
                    at: "songs".into(),
                    message: format!("\"{title}\" was already played in this show, so it can't be removed. Keep it in the file with skip: true."),
                }])),
            ).into_response());
        }
        sqlx::query("DELETE FROM songs WHERE id = ?").bind(id).execute(&mut *tx).await?;
    }

    // Mixes.
    sqlx::query("DELETE FROM show_bases WHERE show_id = ?").bind(show_id).execute(&mut *tx).await?;
    for (txl, mix) in &f.mixes {
        sqlx::query("INSERT INTO show_bases (show_id, base_id, mix) SELECT ?, id, ? FROM equipment WHERE label = ?")
            .bind(show_id).bind(mix).bind(txl).execute(&mut *tx).await?;
    }

    // Plot.
    sqlx::query("DELETE FROM plot_cells WHERE show_id = ?").bind(show_id).execute(&mut *tx).await?;
    for (dev, song, who) in &c.cells {
        sqlx::query("INSERT INTO plot_cells (show_id, device_id, song_key, performer_id) VALUES (?, ?, ?, ?)")
            .bind(show_id)
            .bind(device[dev])
            .bind(song.map(|i| song_ids[i]).unwrap_or(0))
            .bind(who.as_ref().map(|w| person[w]))
            .execute(&mut *tx).await?;
    }

    // Passes: keep ids (and their done marks) for passes that are unchanged.
    let mut keep: Vec<Id> = Vec::new();
    for (i, list) in &c.passes {
        let mut seq_by: BTreeMap<String, i64> = BTreeMap::new();
        for p in list {
            let seq = seq_by.entry(p.device.clone()).or_insert(0);
            *seq += 1;
            let to = p.to.as_ref().map(|w| person[w]);
            let id: Id = sqlx::query_scalar(
                "INSERT INTO passes (show_id, device_id, song_id, seq, to_performer_id) VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(device_id, song_id, seq) DO UPDATE SET to_performer_id = excluded.to_performer_id
                 RETURNING id",
            )
            .bind(show_id).bind(device[&p.device]).bind(song_ids[*i]).bind(*seq).bind(to)
            .fetch_one(&mut *tx).await?;
            keep.push(id);
        }
    }
    let all: Vec<Id> = sqlx::query_scalar("SELECT id FROM passes WHERE show_id = ?").bind(show_id).fetch_all(&mut *tx).await?;
    for id in all.into_iter().filter(|id| !keep.contains(id)) {
        sqlx::query("DELETE FROM passes WHERE id = ?").bind(id).execute(&mut *tx).await?;
    }

    log_event(&mut tx, &actor, "show.imported", &json!({ "show_id": show_id, "name": f.show, "songs": c.songs.len() })).await?;
    tx.commit().await?;
    let mut out = problems_json(&probs);
    out["show_id"] = json!(show_id);
    Ok((if existing.is_some() { StatusCode::OK } else { StatusCode::CREATED }, Json(out)).into_response())
}

/// The show written back out as a show file.
async fn showfile_export(State(s): State<AppState>, Path(id): Path<Id>) -> ApiResult<Response> {
    type ShowRow = (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<Id>, Option<Id>);
    let show: ShowRow = sqlx::query_as(
        "SELECT name, day, set_name, starts_at, call_time, event_id, follows_show_id FROM shows WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    let (st, names) = plan_store::load(&s.pool, id).await?;
    let event: Option<(String, String)> = match show.5 {
        Some(e) => sqlx::query_as("SELECT name, timezone FROM events WHERE id = ?").bind(e).fetch_optional(&s.pool).await?,
        None => None,
    };
    let follows: Option<String> = match show.6 {
        Some(f) => sqlx::query_scalar("SELECT name FROM shows WHERE id = ?").bind(f).fetch_optional(&s.pool).await?,
        None => None,
    };
    let person = |p: Id| names.people.get(&p).cloned().unwrap_or_default();
    let dev = |d: Id| names.devices.get(&d).cloned().unwrap_or_default();

    // People referenced by the plot or passes (and absences), with zone/notes.
    let mut used: std::collections::BTreeSet<Id> = st.cells.values().flatten().copied().collect();
    used.extend(st.passes.iter().filter_map(|p| p.to));
    used.extend(st.absent.iter().copied());
    let mut people = Vec::new();
    for p in &used {
        let (name, zone, notes): (String, Option<String>, String) =
            sqlx::query_as("SELECT name, zone, notes FROM performers WHERE id = ?").bind(p).fetch_one(&s.pool).await?;
        let absent = st.absent.contains(p);
        people.push(if zone.is_none() && notes.is_empty() && !absent {
            showfile::PersonEntry::Name(name)
        } else {
            showfile::PersonEntry::Full(showfile::Person {
                name,
                zone,
                notes: (!notes.is_empty()).then_some(notes),
                absent,
            })
        });
    }
    let songs_rows: Vec<(Id, String, Option<String>, bool, String)> = sqlx::query_as(
        "SELECT id, title, midi_trigger, skipped, notes FROM songs WHERE show_id = ? ORDER BY position",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    let mut songs = Vec::new();
    for (sid, title, trig, skip, notes) in &songs_rows {
        let pc = trig.as_deref().and_then(rfops_core::midi::Trigger::parse_json).and_then(|t| match t {
            rfops_core::midi::Trigger::ProgramChange { value, .. } => Some(value),
            _ => None,
        });
        let (chains, _) = st.chains(*sid, 0);
        let mut passes = Vec::new();
        let mut list: Vec<&rfops_core::plan::Pass> = st.passes.iter().filter(|p| p.song == *sid).collect();
        list.sort_by_key(|p| (p.device, p.seq));
        for p in list {
            let c = &chains[&p.device];
            let from = c.holders[(p.seq - 1) as usize];
            passes.push(format!(
                "{}: {} -> {}",
                dev(p.device),
                from.map(person).unwrap_or_else(|| "nobody".into()),
                p.to.map(person).unwrap_or_else(|| "nobody".into())
            ));
        }
        songs.push(showfile::SongEntry {
            title: title.clone(),
            pc,
            skip: *skip,
            passes,
            notes: (!notes.is_empty()).then(|| notes.clone()),
        });
    }
    let mut plot: BTreeMap<String, BTreeMap<String, Option<String>>> = BTreeMap::new();
    for ((d, song), p) in &st.cells {
        let col = match song {
            None => "default".to_string(),
            Some(sid) => names.songs.get(sid).cloned().unwrap_or_default(),
        };
        plot.entry(col).or_default().insert(dev(*d), Some(p.map(person).unwrap_or_else(|| "nobody".into())));
    }
    let mixes = st
        .base_mix
        .iter()
        .map(|(b, m)| (names.equipment.get(b).cloned().unwrap_or_default(), m.clone()))
        .collect();
    let file = ShowFile {
        show: show.0,
        event: event.as_ref().map(|e| e.0.clone()),
        timezone: event.map(|e| e.1),
        day: show.1,
        set_name: show.2,
        concert: show.3,
        call: show.4,
        follows,
        people,
        devices: vec![],
        mixes,
        songs,
        plot,
    };
    Ok(([(header::CONTENT_TYPE, "text/yaml; charset=utf-8")], showfile::to_yaml(&file)).into_response())
}
