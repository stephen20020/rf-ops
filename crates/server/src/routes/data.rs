//! Backups, show export/import (JSON), and spreadsheet round-trip (CSV) for inventory and
//! performers, plus a mic-plot CSV export.

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rfops_core::{assign, EquipmentKind, EquipmentStatus, KitKind};
use serde::{Deserialize, Serialize};

use crate::{
    backup::{self, BackupFile},
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    routes::{
        inventory::Equipment,
        shows::{load_plan, Show},
    },
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/backups", get(list_backups).post(create_backup))
        .route("/api/shows/{id}/export", get(export_show))
        .route("/api/shows/import", post(import_show))
        .route("/api/shows/{id}/export/plot.csv", get(plot_csv))
        .route("/api/export/equipment.csv", get(equipment_csv))
        .route("/api/import/equipment", post(import_equipment))
        .route("/api/export/performers.csv", get(performers_csv))
        .route("/api/import/performers", post(import_performers))
}

// ---------------- backups ----------------

async fn list_backups(State(s): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let Some(dir) = &s.backup_dir else {
        return Ok(Json(serde_json::json!({ "enabled": false, "files": [] })));
    };
    Ok(Json(serde_json::json!({
        "enabled": true,
        "dir": dir.display().to_string(),
        "files": backup::list(dir),
    })))
}

async fn create_backup(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<BackupFile>> {
    let dir = s
        .backup_dir
        .as_ref()
        .ok_or_else(|| ApiError::bad("backups are not enabled for this database"))?;
    let f = backup::run(&s.pool, dir, "manual")
        .await
        .map_err(ApiError::BadRequest)?;
    let mut tx = s.pool.begin().await?;
    log_event(&mut tx, &actor, "backup.created", &f).await?;
    tx.commit().await?;
    Ok(Json(f))
}

// ---------------- show export / import ----------------

pub const SHOW_FORMAT: &str = "rfops-show/1";

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowFile {
    pub format: String,
    #[serde(default)]
    pub exported_at: String,
    pub show: ShowMeta,
    pub songs: Vec<SongOut>,
    pub kits: Vec<KitOut>,
    pub assignments: Vec<AssignmentOut>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowMeta {
    pub name: String,
    pub starts_at: Option<String>,
    pub call_time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SongOut {
    pub position: i64,
    pub title: String,
    pub midi_trigger: Option<serde_json::Value>,
    #[serde(default)]
    pub skipped: bool,
}

/// Equipment is referenced by label so files move between databases.
#[derive(Debug, Serialize, Deserialize)]
pub struct KitOut {
    pub kind: String,
    pub name: String,
    pub base: Option<String>,
    pub carrier: Option<String>,
    pub element: Option<String>,
    pub console_channel: Option<i64>,
    pub console_channel_name: Option<String>,
    pub dca: Option<String>,
    pub iem_mix: Option<String>,
    #[serde(default)]
    pub is_spare: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssignmentOut {
    pub performer: String,
    pub kind: String,
    /// Kit name; `None` = "no kit of this kind in this song".
    pub kit: Option<String>,
    /// Song position; `None` = show default.
    pub song: Option<i64>,
}

async fn export_show(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult<Response> {
    let plan = load_plan(&s.pool, id).await?;
    let labels: HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, label FROM equipment")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let performers: HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM performers")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let label = |id: Option<i64>| id.and_then(|i| labels.get(&i).cloned());
    let kit_names: HashMap<i64, String> =
        plan.kits.iter().map(|k| (k.id, k.name.clone())).collect();
    let song_pos: HashMap<i64, i64> = plan.songs.iter().map(|s| (s.id, s.position)).collect();
    let file = ShowFile {
        format: SHOW_FORMAT.into(),
        exported_at: now(),
        show: ShowMeta {
            name: plan.show.name.clone(),
            starts_at: plan.show.starts_at.clone(),
            call_time: plan.show.call_time.clone(),
        },
        songs: plan
            .songs
            .iter()
            .map(|s| SongOut {
                position: s.position,
                title: s.title.clone(),
                midi_trigger: s
                    .midi_trigger
                    .as_deref()
                    .and_then(|t| serde_json::from_str(t).ok()),
                skipped: s.skipped,
            })
            .collect(),
        kits: plan
            .kits
            .iter()
            .map(|k| KitOut {
                kind: k.kind.clone(),
                name: k.name.clone(),
                base: label(k.base_id),
                carrier: label(k.carrier_id),
                element: label(k.element_id),
                console_channel: k.console_channel,
                console_channel_name: k.console_channel_name.clone(),
                dca: k.dca.clone(),
                iem_mix: k.iem_mix.clone(),
                is_spare: k.is_spare,
            })
            .collect(),
        assignments: plan
            .assignments
            .iter()
            .map(|a| AssignmentOut {
                performer: performers.get(&a.performer_id).cloned().unwrap_or_default(),
                kind: a.kind.clone(),
                kit: a.kit_id.and_then(|k| kit_names.get(&k).cloned()),
                song: a.song_id.and_then(|s| song_pos.get(&s).copied()),
            })
            .collect(),
    };
    let filename = sanitize(&plan.show.name);
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}.rfops.json\""),
            ),
        ],
        serde_json::to_string_pretty(&file).unwrap_or_default(),
    )
        .into_response())
}

fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() {
        "show".into()
    } else {
        s
    }
}

#[derive(Debug, Deserialize)]
pub struct ImportShow {
    pub file: ShowFile,
    /// Overrides the show name in the file.
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct ImportShowResult {
    show: Show,
    performers_created: Vec<String>,
}

async fn import_show(
    State(s): State<AppState>,
    actor: Actor,
    Json(req): Json<ImportShow>,
) -> ApiResult<(StatusCode, Json<ImportShowResult>)> {
    let f = req.file;
    if f.format != SHOW_FORMAT {
        return Err(ApiError::bad(format!(
            "unsupported file format '{}' (expected {SHOW_FORMAT})",
            f.format
        )));
    }
    let eq: HashMap<String, (i64, String)> =
        sqlx::query_as::<_, (i64, String, String)>("SELECT id, label, kind FROM equipment")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .map(|(id, l, k)| (l, (id, k)))
            .collect();
    let mut missing: Vec<String> = f
        .kits
        .iter()
        .flat_map(|k| [&k.base, &k.carrier, &k.element])
        .flatten()
        .filter(|l| !eq.contains_key(*l))
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    if !missing.is_empty() {
        return Err(ApiError::bad(format!(
            "these equipment labels are not in the inventory: {}",
            missing.join(", ")
        )));
    }
    for k in &f.kits {
        KitKind::parse(&k.kind)
            .ok_or_else(|| ApiError::bad(format!("kit {}: unknown kind {}", k.name, k.kind)))?;
    }

    let mut tx = s.pool.begin().await?;
    let name = req.name.unwrap_or(f.show.name.clone());
    let show_id = sqlx::query("INSERT INTO shows (name, starts_at, call_time) VALUES (?, ?, ?)")
        .bind(name.trim())
        .bind(&f.show.starts_at)
        .bind(&f.show.call_time)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    let mut songs = HashMap::new();
    let mut ordered: Vec<&SongOut> = f.songs.iter().collect();
    ordered.sort_by_key(|s| s.position);
    for (i, song) in ordered.iter().enumerate() {
        let id = sqlx::query("INSERT INTO songs (show_id, position, title, midi_trigger, skipped) VALUES (?, ?, ?, ?, ?)")
            .bind(show_id)
            .bind(i as i64 + 1)
            .bind(&song.title)
            .bind(song.midi_trigger.as_ref().map(|v| v.to_string()))
            .bind(song.skipped)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        songs.insert(song.position, id);
    }
    let lookup = |l: &Option<String>| l.as_ref().map(|l| eq[l].0);
    let mut kits = HashMap::new();
    for k in &f.kits {
        let id = sqlx::query(
            "INSERT INTO kits (show_id, kind, name, base_id, carrier_id, element_id, console_channel, console_channel_name, dca, iem_mix, is_spare)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(show_id)
        .bind(&k.kind)
        .bind(&k.name)
        .bind(lookup(&k.base))
        .bind(lookup(&k.carrier))
        .bind(lookup(&k.element))
        .bind(k.console_channel)
        .bind(&k.console_channel_name)
        .bind(&k.dca)
        .bind(&k.iem_mix)
        .bind(k.is_spare)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        kits.insert(k.name.clone(), id);
    }
    let mut performers: HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT name, id FROM performers")
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
    let mut created = Vec::new();
    for a in &f.assignments {
        let pid = match performers.get(&a.performer) {
            Some(id) => *id,
            None => {
                let id = sqlx::query("INSERT INTO performers (name) VALUES (?)")
                    .bind(&a.performer)
                    .execute(&mut *tx)
                    .await?
                    .last_insert_rowid();
                performers.insert(a.performer.clone(), id);
                created.push(a.performer.clone());
                id
            }
        };
        let kit =
            match &a.kit {
                Some(n) => Some(*kits.get(n).ok_or_else(|| {
                    ApiError::bad(format!("assignment refers to unknown kit '{n}'"))
                })?),
                None => None,
            };
        let song = match a.song {
            Some(p) => Some(*songs.get(&p).ok_or_else(|| {
                ApiError::bad(format!("assignment refers to missing song position {p}"))
            })?),
            None => None,
        };
        sqlx::query("INSERT INTO assignments (show_id, performer_id, kind, kit_id, song_id) VALUES (?, ?, ?, ?, ?)")
            .bind(show_id)
            .bind(pid)
            .bind(&a.kind)
            .bind(kit)
            .bind(song)
            .execute(&mut *tx)
            .await?;
    }
    let show = sqlx::query_as::<_, Show>("SELECT * FROM shows WHERE id = ?")
        .bind(show_id)
        .fetch_one(&mut *tx)
        .await?;
    log_event(
        &mut tx,
        &actor,
        "show.imported",
        &serde_json::json!({ "show": show, "performers_created": created }),
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(ImportShowResult {
            show,
            performers_created: created,
        }),
    ))
}

// ---------------- CSV ----------------

fn csv_response(name: &str, body: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        body,
    )
        .into_response()
}

fn writer() -> csv::Writer<Vec<u8>> {
    csv::Writer::from_writer(Vec::new())
}

fn finish(w: csv::Writer<Vec<u8>>) -> ApiResult<Vec<u8>> {
    w.into_inner().map_err(|e| ApiError::bad(e.to_string()))
}

#[derive(Debug, Serialize, Deserialize)]
struct EquipmentCsv {
    kind: String,
    #[serde(default)]
    make: String,
    #[serde(default)]
    model: String,
    label: String,
    #[serde(default)]
    serial: String,
    #[serde(default)]
    parent: String,
    #[serde(default)]
    channel_no: String,
    #[serde(default)]
    ip_address: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    notes: String,
}

async fn equipment_csv(State(s): State<AppState>) -> ApiResult<Response> {
    let rows = sqlx::query_as::<_, Equipment>("SELECT * FROM equipment ORDER BY kind, label")
        .fetch_all(&s.pool)
        .await?;
    let labels: HashMap<i64, String> = rows.iter().map(|e| (e.id, e.label.clone())).collect();
    let mut w = writer();
    for e in rows {
        w.serialize(EquipmentCsv {
            kind: e.kind,
            make: e.make,
            model: e.model,
            label: e.label,
            serial: e.serial.unwrap_or_default(),
            parent: e
                .parent_id
                .and_then(|p| labels.get(&p).cloned())
                .unwrap_or_default(),
            channel_no: e.channel_no.map(|c| c.to_string()).unwrap_or_default(),
            ip_address: e.ip_address.unwrap_or_default(),
            status: e.status,
            notes: e.notes,
        })
        .map_err(|e| ApiError::bad(e.to_string()))?;
    }
    Ok(csv_response("equipment.csv", finish(w)?))
}

#[derive(Serialize, Default)]
pub struct ImportResult {
    created: usize,
    updated: usize,
    errors: Vec<String>,
}

fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Upsert equipment by label. All-or-nothing: any error rejects the whole file.
async fn import_equipment(
    State(s): State<AppState>,
    actor: Actor,
    body: String,
) -> ApiResult<Response> {
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(body.as_bytes());
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (i, r) in rdr.deserialize::<EquipmentCsv>().enumerate() {
        let line = i + 2;
        match r {
            Err(e) => errors.push(format!("line {line}: {e}")),
            Ok(r) => {
                if EquipmentKind::parse(&r.kind).is_none() {
                    errors.push(format!("line {line}: unknown kind '{}'", r.kind));
                } else if r.label.is_empty() {
                    errors.push(format!("line {line}: label is required"));
                } else if !r.status.is_empty() && EquipmentStatus::parse(&r.status).is_none() {
                    errors.push(format!("line {line}: unknown status '{}'", r.status));
                } else if !r.channel_no.is_empty() && r.channel_no.parse::<i64>().is_err() {
                    errors.push(format!("line {line}: channel_no must be a number"));
                } else {
                    rows.push((line, r));
                }
            }
        }
    }
    let existing: HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT label, id FROM equipment")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let all_labels: std::collections::HashSet<&str> = existing
        .keys()
        .map(String::as_str)
        .chain(rows.iter().map(|(_, r)| r.label.as_str()))
        .collect();
    for (line, r) in &rows {
        if !r.parent.is_empty() && !all_labels.contains(r.parent.as_str()) {
            errors.push(format!("line {line}: parent '{}' not found", r.parent));
        }
    }
    if !errors.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(ImportResult {
                errors,
                ..Default::default()
            }),
        )
            .into_response());
    }

    let mut tx = s.pool.begin().await?;
    let mut result = ImportResult::default();
    let mut ids = existing.clone();
    // Pass 1: upsert without parents.
    for (_, r) in &rows {
        let status = opt(&r.status).unwrap_or_else(|| "available".into());
        let channel = r.channel_no.parse::<i64>().ok();
        if let Some(id) = existing.get(&r.label) {
            sqlx::query("UPDATE equipment SET kind=?, make=?, model=?, serial=?, channel_no=?, ip_address=?, status=?, notes=? WHERE id=?")
                .bind(&r.kind)
                .bind(&r.make)
                .bind(&r.model)
                .bind(opt(&r.serial))
                .bind(channel)
                .bind(opt(&r.ip_address))
                .bind(&status)
                .bind(&r.notes)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            result.updated += 1;
        } else {
            let id = sqlx::query("INSERT INTO equipment (kind, make, model, label, serial, channel_no, ip_address, status, notes) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&r.kind)
                .bind(&r.make)
                .bind(&r.model)
                .bind(&r.label)
                .bind(opt(&r.serial))
                .bind(channel)
                .bind(opt(&r.ip_address))
                .bind(&status)
                .bind(&r.notes)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid();
            ids.insert(r.label.clone(), id);
            result.created += 1;
        }
    }
    // Pass 2: parents.
    for (_, r) in &rows {
        let parent = opt(&r.parent).and_then(|p| ids.get(&p).copied());
        sqlx::query("UPDATE equipment SET parent_id = ? WHERE id = ?")
            .bind(parent)
            .bind(ids[&r.label])
            .execute(&mut *tx)
            .await?;
    }
    log_event(
        &mut tx,
        &actor,
        "equipment.imported",
        &serde_json::json!({ "created": result.created, "updated": result.updated }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(result).into_response())
}

#[derive(Debug, Serialize, Deserialize)]
struct PerformerCsv {
    name: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    zone: String,
    #[serde(default)]
    a2: String,
    #[serde(default)]
    notes: String,
}

/// name, role, zone, A2 name, notes
type PerformerExportRow = (String, String, Option<String>, Option<String>, String);

async fn performers_csv(State(s): State<AppState>) -> ApiResult<Response> {
    let rows: Vec<PerformerExportRow> = sqlx::query_as(
        "SELECT p.name, p.role, p.zone, u.name, p.notes FROM performers p LEFT JOIN users u ON u.id = p.owner_user_id ORDER BY p.name",
    )
    .fetch_all(&s.pool)
    .await?;
    let mut w = writer();
    for (name, role, zone, a2, notes) in rows {
        w.serialize(PerformerCsv {
            name,
            role,
            zone: zone.unwrap_or_default(),
            a2: a2.unwrap_or_default(),
            notes,
        })
        .map_err(|e| ApiError::bad(e.to_string()))?;
    }
    Ok(csv_response("performers.csv", finish(w)?))
}

async fn import_performers(
    State(s): State<AppState>,
    actor: Actor,
    body: String,
) -> ApiResult<Response> {
    let users: HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT name, id FROM users")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(body.as_bytes());
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (i, r) in rdr.deserialize::<PerformerCsv>().enumerate() {
        let line = i + 2;
        match r {
            Err(e) => errors.push(format!("line {line}: {e}")),
            Ok(r) if r.name.is_empty() => errors.push(format!("line {line}: name is required")),
            Ok(r) if !r.a2.is_empty() && !users.contains_key(&r.a2) => {
                errors.push(format!("line {line}: operator '{}' not found", r.a2))
            }
            Ok(r) => rows.push(r),
        }
    }
    if !errors.is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(ImportResult {
                errors,
                ..Default::default()
            }),
        )
            .into_response());
    }
    let existing: HashMap<String, i64> =
        sqlx::query_as::<_, (String, i64)>("SELECT name, id FROM performers")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let mut tx = s.pool.begin().await?;
    let mut result = ImportResult::default();
    for r in &rows {
        let owner = opt(&r.a2).and_then(|a| users.get(&a).copied());
        if let Some(id) = existing.get(&r.name) {
            sqlx::query(
                "UPDATE performers SET role=?, zone=?, owner_user_id=?, notes=? WHERE id=?",
            )
            .bind(&r.role)
            .bind(opt(&r.zone))
            .bind(owner)
            .bind(&r.notes)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            result.updated += 1;
        } else {
            sqlx::query("INSERT INTO performers (name, role, zone, owner_user_id, notes) VALUES (?, ?, ?, ?, ?)")
                .bind(&r.name)
                .bind(&r.role)
                .bind(opt(&r.zone))
                .bind(owner)
                .bind(&r.notes)
                .execute(&mut *tx)
                .await?;
            result.created += 1;
        }
    }
    log_event(
        &mut tx,
        &actor,
        "performers.imported",
        &serde_json::json!({ "created": result.created, "updated": result.updated }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(result).into_response())
}

#[derive(Serialize)]
struct PlotRow<'a> {
    position: i64,
    song: &'a str,
    performer: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    kit: &'a str,
    console: String,
    receiver: String,
    pack: String,
    element: String,
}

/// Mic plot: who has what in every song (read-only export).
async fn plot_csv(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult<Response> {
    let plan = load_plan(&s.pool, id).await?;
    let labels: HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, label FROM equipment")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let performers: HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM performers")
            .fetch_all(&s.pool)
            .await?
            .into_iter()
            .collect();
    let lab = |i: Option<i64>| i.and_then(|i| labels.get(&i).cloned()).unwrap_or_default();
    let assignments = plan.core_assignments();
    let mut w = writer();
    for song in plan.songs.iter().filter(|s| !s.skipped) {
        let eff = assign::effective_assignment(&assignments, Some(song.id));
        for (p, kit_ids) in &eff {
            for k in kit_ids {
                let Some(kit) = plan.kits.iter().find(|x| x.id == *k) else {
                    continue;
                };
                w.serialize(PlotRow {
                    position: song.position,
                    song: &song.title,
                    performer: performers.get(p).map(String::as_str).unwrap_or(""),
                    kind: &kit.kind,
                    kit: &kit.name,
                    console: if kit.kind == "iem" {
                        kit.iem_mix.clone().unwrap_or_default()
                    } else {
                        kit.console_channel
                            .map(|c| format!("Ch {c}"))
                            .unwrap_or_default()
                    },
                    receiver: lab(kit.base_id),
                    pack: lab(kit.carrier_id),
                    element: lab(kit.element_id),
                })
                .map_err(|e| ApiError::bad(e.to_string()))?;
            }
        }
    }
    Ok(csv_response(
        &format!("{}-plot.csv", sanitize(&plan.show.name)),
        finish(w)?,
    ))
}
