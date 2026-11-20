//! Equipment, users and performers.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use rfops_core::{EquipmentKind, EquipmentStatus};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    db::{log_event, Actor},
    double_option,
    error::{ApiError, ApiResult},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/equipment", get(list_equipment).post(create_equipment))
        .route(
            "/api/equipment/{id}",
            get(get_equipment)
                .patch(update_equipment)
                .delete(delete_equipment),
        )
        .route("/api/users", get(list_users).post(create_user))
        .route(
            "/api/performers",
            get(list_performers).post(create_performer),
        )
        .route(
            "/api/performers/{id}",
            get(get_performer)
                .patch(update_performer)
                .delete(delete_performer),
        )
}

// ---------------- equipment ----------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Equipment {
    pub id: i64,
    pub kind: String,
    pub make: String,
    pub model: String,
    pub label: String,
    pub serial: Option<String>,
    pub parent_id: Option<i64>,
    pub channel_no: Option<i64>,
    pub ip_address: Option<String>,
    pub status: String,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct NewEquipment {
    pub kind: String,
    #[serde(default)]
    pub make: String,
    #[serde(default)]
    pub model: String,
    pub label: String,
    pub serial: Option<String>,
    pub parent_id: Option<i64>,
    pub channel_no: Option<i64>,
    pub ip_address: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct EquipmentPatch {
    pub make: Option<String>,
    pub model: Option<String>,
    pub label: Option<String>,
    #[serde(default, with = "double_option")]
    pub serial: Option<Option<String>>,
    #[serde(default, with = "double_option")]
    pub parent_id: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub channel_no: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub ip_address: Option<Option<String>>,
    pub status: Option<String>,
    pub notes: Option<String>,
}

fn check_kind(kind: &str) -> ApiResult<()> {
    EquipmentKind::parse(kind)
        .map(|_| ())
        .ok_or_else(|| ApiError::bad(format!("unknown equipment kind '{kind}'")))
}

fn check_status(status: &str) -> ApiResult<()> {
    EquipmentStatus::parse(status)
        .map(|_| ())
        .ok_or_else(|| ApiError::bad(format!("unknown equipment status '{status}'")))
}

#[derive(Debug, Deserialize)]
pub struct EquipmentFilter {
    kind: Option<String>,
    status: Option<String>,
}

async fn list_equipment(
    State(s): State<AppState>,
    Query(f): Query<EquipmentFilter>,
) -> ApiResult<Json<Vec<Equipment>>> {
    let rows = sqlx::query_as::<_, Equipment>(
        "SELECT * FROM equipment WHERE (?1 IS NULL OR kind = ?1) AND (?2 IS NULL OR status = ?2) ORDER BY kind, label",
    )
    .bind(f.kind)
    .bind(f.status)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(rows))
}

pub(crate) async fn fetch_equipment<'e, E: sqlx::SqliteExecutor<'e>>(
    e: E,
    id: i64,
) -> ApiResult<Equipment> {
    sqlx::query_as::<_, Equipment>("SELECT * FROM equipment WHERE id = ?")
        .bind(id)
        .fetch_optional(e)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn get_equipment(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Equipment>> {
    Ok(Json(fetch_equipment(&s.pool, id).await?))
}

async fn create_equipment(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewEquipment>,
) -> ApiResult<(StatusCode, Json<Equipment>)> {
    check_kind(&n.kind)?;
    let status = n.status.unwrap_or_else(|| "available".into());
    check_status(&status)?;
    if n.label.trim().is_empty() {
        return Err(ApiError::bad("label is required"));
    }
    let mut tx = s.pool.begin().await?;
    let id = sqlx::query(
        "INSERT INTO equipment (kind, make, model, label, serial, parent_id, channel_no, ip_address, status, notes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&n.kind)
    .bind(&n.make)
    .bind(&n.model)
    .bind(n.label.trim())
    .bind(&n.serial)
    .bind(n.parent_id)
    .bind(n.channel_no)
    .bind(&n.ip_address)
    .bind(&status)
    .bind(&n.notes)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let row = fetch_equipment(&mut *tx, id).await?;
    log_event(&mut tx, &actor, "equipment.created", &row).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn update_equipment(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(p): Json<EquipmentPatch>,
) -> ApiResult<Json<Equipment>> {
    let mut tx = s.pool.begin().await?;
    let mut e = fetch_equipment(&mut *tx, id).await?;
    if let Some(v) = p.make {
        e.make = v;
    }
    if let Some(v) = p.model {
        e.model = v;
    }
    if let Some(v) = p.label {
        if v.trim().is_empty() {
            return Err(ApiError::bad("label is required"));
        }
        e.label = v.trim().into();
    }
    if let Some(v) = p.serial {
        e.serial = v;
    }
    if let Some(v) = p.parent_id {
        e.parent_id = v;
    }
    if let Some(v) = p.channel_no {
        e.channel_no = v;
    }
    if let Some(v) = p.ip_address {
        e.ip_address = v;
    }
    if let Some(v) = p.status {
        check_status(&v)?;
        e.status = v;
    }
    if let Some(v) = p.notes {
        e.notes = v;
    }
    sqlx::query(
        "UPDATE equipment SET make=?, model=?, label=?, serial=?, parent_id=?, channel_no=?, ip_address=?, status=?, notes=? WHERE id=?",
    )
    .bind(&e.make)
    .bind(&e.model)
    .bind(&e.label)
    .bind(&e.serial)
    .bind(e.parent_id)
    .bind(e.channel_no)
    .bind(&e.ip_address)
    .bind(&e.status)
    .bind(&e.notes)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    log_event(&mut tx, &actor, "equipment.updated", &e).await?;
    tx.commit().await?;
    Ok(Json(e))
}

async fn delete_equipment(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let e = fetch_equipment(&mut *tx, id).await?;
    let in_kit: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM kits WHERE base_id = ?1 OR carrier_id = ?1 OR element_id = ?1 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if in_kit.is_some() {
        return Err(ApiError::Conflict(
            "equipment is used in a kit; retire it instead".into(),
        ));
    }
    sqlx::query("DELETE FROM equipment WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "equipment.deleted", &e).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------- users ----------------

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct NewUser {
    pub name: String,
    pub role: Option<String>,
}

const ROLES: [&str; 4] = ["admin", "a2", "foh", "viewer"];

async fn list_users(State(s): State<AppState>) -> ApiResult<Json<Vec<User>>> {
    Ok(Json(
        sqlx::query_as::<_, User>("SELECT id, name, role FROM users ORDER BY name")
            .fetch_all(&s.pool)
            .await?,
    ))
}

async fn create_user(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewUser>,
) -> ApiResult<(StatusCode, Json<User>)> {
    let role = n.role.unwrap_or_else(|| "a2".into());
    if !ROLES.contains(&role.as_str()) {
        return Err(ApiError::bad(format!("role must be one of {ROLES:?}")));
    }
    if n.name.trim().is_empty() {
        return Err(ApiError::bad("name is required"));
    }
    let mut tx = s.pool.begin().await?;
    let id = sqlx::query("INSERT INTO users (name, role) VALUES (?, ?)")
        .bind(n.name.trim())
        .bind(&role)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    let user = User {
        id,
        name: n.name.trim().into(),
        role,
    };
    log_event(&mut tx, &actor, "user.created", &user).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(user)))
}

// ---------------- performers ----------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Performer {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub photo_path: Option<String>,
    pub notes: String,
    pub owner_user_id: Option<i64>,
    pub zone: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NewPerformer {
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub notes: String,
    pub owner_user_id: Option<i64>,
    pub zone: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PerformerPatch {
    pub name: Option<String>,
    pub role: Option<String>,
    pub notes: Option<String>,
    #[serde(default, with = "double_option")]
    pub owner_user_id: Option<Option<i64>>,
    #[serde(default, with = "double_option")]
    pub zone: Option<Option<String>>,
}

async fn fetch_performer<'e, E: sqlx::SqliteExecutor<'e>>(e: E, id: i64) -> ApiResult<Performer> {
    sqlx::query_as::<_, Performer>("SELECT * FROM performers WHERE id = ?")
        .bind(id)
        .fetch_optional(e)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn list_performers(State(s): State<AppState>) -> ApiResult<Json<Vec<Performer>>> {
    Ok(Json(
        sqlx::query_as::<_, Performer>("SELECT * FROM performers ORDER BY name")
            .fetch_all(&s.pool)
            .await?,
    ))
}

async fn get_performer(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Performer>> {
    Ok(Json(fetch_performer(&s.pool, id).await?))
}

async fn create_performer(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewPerformer>,
) -> ApiResult<(StatusCode, Json<Performer>)> {
    if n.name.trim().is_empty() {
        return Err(ApiError::bad("name is required"));
    }
    let mut tx = s.pool.begin().await?;
    let id = sqlx::query(
        "INSERT INTO performers (name, role, notes, owner_user_id, zone) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(n.name.trim())
    .bind(&n.role)
    .bind(&n.notes)
    .bind(n.owner_user_id)
    .bind(&n.zone)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let p = fetch_performer(&mut *tx, id).await?;
    log_event(&mut tx, &actor, "performer.created", &p).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(p)))
}

async fn update_performer(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(patch): Json<PerformerPatch>,
) -> ApiResult<Json<Performer>> {
    let mut tx = s.pool.begin().await?;
    let mut p = fetch_performer(&mut *tx, id).await?;
    if let Some(v) = patch.name {
        if v.trim().is_empty() {
            return Err(ApiError::bad("name is required"));
        }
        p.name = v.trim().into();
    }
    if let Some(v) = patch.role {
        p.role = v;
    }
    if let Some(v) = patch.notes {
        p.notes = v;
    }
    if let Some(v) = patch.owner_user_id {
        p.owner_user_id = v;
    }
    if let Some(v) = patch.zone {
        p.zone = v;
    }
    sqlx::query(
        "UPDATE performers SET name=?, role=?, notes=?, owner_user_id=?, zone=? WHERE id=?",
    )
    .bind(&p.name)
    .bind(&p.role)
    .bind(&p.notes)
    .bind(p.owner_user_id)
    .bind(&p.zone)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    log_event(&mut tx, &actor, "performer.updated", &p).await?;
    tx.commit().await?;
    Ok(Json(p))
}

async fn delete_performer(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let mut tx = s.pool.begin().await?;
    let p = fetch_performer(&mut *tx, id).await?;
    sqlx::query("DELETE FROM performers WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "performer.deleted", &p).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
