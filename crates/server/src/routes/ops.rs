//! Checkout/return, battery checks, alerts and the event log.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use rfops_core::{
    alert::{self, AlertAction, AlertState, Severity},
    battery::{battery_due, BatteryCheckRef, BatteryDue, BatteryResult, BatteryRule},
    EquipmentKind,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{
    db::{log_event, now, Actor},
    error::{ApiError, ApiResult},
    routes::{inventory::fetch_equipment, shows::fetch_show},
    AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/checkouts", get(list_checkouts).post(checkout))
        .route("/api/checkouts/{id}/return", post(return_checkout))
        .route(
            "/api/battery-checks",
            get(list_battery_checks).post(record_battery_check),
        )
        .route("/api/shows/{id}/battery-status", get(battery_status))
        .route("/api/alerts", get(list_alerts).post(create_alert))
        .route("/api/alerts/{id}/ack", post(ack_alert))
        .route("/api/alerts/{id}/resolve", post(resolve_alert))
        .route("/api/events", get(list_events))
}

// ---------------- checkout / return ----------------

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Checkout {
    pub id: i64,
    pub equipment_id: i64,
    pub performer_id: Option<i64>,
    pub show_id: Option<i64>,
    pub out_at: String,
    pub out_by: String,
    pub in_at: Option<String>,
    pub in_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NewCheckout {
    pub equipment_id: i64,
    pub performer_id: Option<i64>,
    pub show_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CheckoutFilter {
    #[serde(default)]
    open: bool,
    show_id: Option<i64>,
}

async fn list_checkouts(
    State(s): State<AppState>,
    Query(f): Query<CheckoutFilter>,
) -> ApiResult<Json<Vec<Checkout>>> {
    Ok(Json(
        sqlx::query_as::<_, Checkout>(
            "SELECT * FROM checkouts WHERE (?1 = 0 OR in_at IS NULL) AND (?2 IS NULL OR show_id = ?2) ORDER BY out_at DESC",
        )
        .bind(f.open)
        .bind(f.show_id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

async fn checkout(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewCheckout>,
) -> ApiResult<(StatusCode, Json<Checkout>)> {
    let mut tx = s.pool.begin().await?;
    let eq = fetch_equipment(&mut *tx, n.equipment_id).await?;
    if eq.status == "faulty" || eq.status == "retired" {
        return Err(ApiError::Conflict(format!("{} is {}", eq.label, eq.status)));
    }
    let open: Option<i64> =
        sqlx::query_scalar("SELECT id FROM checkouts WHERE equipment_id = ? AND in_at IS NULL")
            .bind(n.equipment_id)
            .fetch_optional(&mut *tx)
            .await?;
    if open.is_some() {
        return Err(ApiError::Conflict(format!(
            "{} is already checked out",
            eq.label
        )));
    }
    let id = sqlx::query("INSERT INTO checkouts (equipment_id, performer_id, show_id, out_at, out_by) VALUES (?, ?, ?, ?, ?)")
        .bind(n.equipment_id)
        .bind(n.performer_id)
        .bind(n.show_id)
        .bind(now())
        .bind(&actor.name)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
    sqlx::query("UPDATE equipment SET status = 'checked_out' WHERE id = ?")
        .bind(n.equipment_id)
        .execute(&mut *tx)
        .await?;
    let c = sqlx::query_as::<_, Checkout>("SELECT * FROM checkouts WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "checkout.out", &c).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(c)))
}

#[derive(Debug, Default, Deserialize)]
pub struct ReturnBody {
    /// Mark the item faulty on return instead of available.
    #[serde(default)]
    pub faulty: bool,
}

async fn return_checkout(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    body: Option<Json<ReturnBody>>,
) -> ApiResult<Json<Checkout>> {
    let faulty = body.map(|b| b.faulty).unwrap_or(false);
    let mut tx = s.pool.begin().await?;
    let c = sqlx::query_as::<_, Checkout>("SELECT * FROM checkouts WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    if c.in_at.is_some() {
        return Err(ApiError::Conflict("already returned".into()));
    }
    sqlx::query("UPDATE checkouts SET in_at = ?, in_by = ? WHERE id = ?")
        .bind(now())
        .bind(&actor.name)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE equipment SET status = ? WHERE id = ?")
        .bind(if faulty { "faulty" } else { "available" })
        .bind(c.equipment_id)
        .execute(&mut *tx)
        .await?;
    let c = sqlx::query_as::<_, Checkout>("SELECT * FROM checkouts WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    log_event(
        &mut tx,
        &actor,
        "checkout.returned",
        &serde_json::json!({ "checkout": c, "faulty": faulty }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(c))
}

// ---------------- battery checks ----------------

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct BatteryCheck {
    pub id: i64,
    pub equipment_id: i64,
    pub result: String,
    pub battery_type: String,
    pub battery_id: Option<i64>,
    pub checked_at: String,
    pub checked_by: String,
    pub source: String,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct NewBatteryCheck {
    pub equipment_id: i64,
    pub result: String,
    #[serde(default)]
    pub battery_type: String,
    pub battery_id: Option<i64>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct BatteryFilter {
    equipment_id: Option<i64>,
}

async fn list_battery_checks(
    State(s): State<AppState>,
    Query(f): Query<BatteryFilter>,
) -> ApiResult<Json<Vec<BatteryCheck>>> {
    Ok(Json(
        sqlx::query_as::<_, BatteryCheck>(
            "SELECT * FROM battery_checks WHERE (?1 IS NULL OR equipment_id = ?1) ORDER BY checked_at DESC LIMIT 500",
        )
        .bind(f.equipment_id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

async fn record_battery_check(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewBatteryCheck>,
) -> ApiResult<(StatusCode, Json<BatteryCheck>)> {
    BatteryResult::parse(&n.result)
        .ok_or_else(|| ApiError::bad("result must be fresh, ok, replaced or low"))?;
    let mut tx = s.pool.begin().await?;
    let eq = fetch_equipment(&mut *tx, n.equipment_id).await?;
    if !EquipmentKind::parse(&eq.kind).is_some_and(|k| k.needs_battery()) {
        return Err(ApiError::bad(format!(
            "{} ({}) does not take batteries",
            eq.label, eq.kind
        )));
    }
    let id = sqlx::query(
        "INSERT INTO battery_checks (equipment_id, result, battery_type, battery_id, checked_at, checked_by, source, notes)
         VALUES (?, ?, ?, ?, ?, ?, 'manual', ?)",
    )
    .bind(n.equipment_id)
    .bind(&n.result)
    .bind(&n.battery_type)
    .bind(n.battery_id)
    .bind(now())
    .bind(&actor.name)
    .bind(&n.notes)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let c = sqlx::query_as::<_, BatteryCheck>("SELECT * FROM battery_checks WHERE id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    log_event(&mut tx, &actor, "battery.checked", &c).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(c)))
}

#[derive(Debug, Serialize)]
pub struct BatteryStatus {
    pub equipment_id: i64,
    pub label: String,
    pub kind: String,
    pub due: BatteryDue,
    pub last_result: Option<String>,
    pub last_checked_at: Option<String>,
    pub last_checked_by: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatteryStatusQuery {
    /// Override the default max age (minutes) for this request.
    max_age_minutes: Option<i64>,
}

#[derive(FromRow)]
struct BatteryRow {
    id: i64,
    label: String,
    kind: String,
    result: Option<String>,
    checked_at: Option<String>,
    checked_by: Option<String>,
}

/// Battery status of every battery-powered item used in the show's kits.
async fn battery_status(
    State(s): State<AppState>,
    Path(show_id): Path<i64>,
    Query(q): Query<BatteryStatusQuery>,
) -> ApiResult<Json<Vec<BatteryStatus>>> {
    let show = fetch_show(&s.pool, show_id).await?;
    let rows = sqlx::query_as::<_, BatteryRow>(
        "SELECT e.id, e.label, e.kind, bc.result, bc.checked_at, bc.checked_by
         FROM equipment e
         LEFT JOIN battery_checks bc ON bc.id = (
             SELECT id FROM battery_checks WHERE equipment_id = e.id ORDER BY checked_at DESC, id DESC LIMIT 1)
         WHERE e.id IN (SELECT carrier_id FROM kits WHERE show_id = ? AND carrier_id IS NOT NULL)
         ORDER BY e.label",
    )
    .bind(show_id)
    .fetch_all(&s.pool)
    .await?;

    let parse_time = |t: &str| {
        DateTime::parse_from_rfc3339(t)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    };
    let rule = BatteryRule {
        required_since: show.call_time.as_deref().and_then(parse_time),
        max_age_minutes: q.max_age_minutes.or(BatteryRule::default().max_age_minutes),
        ..BatteryRule::default()
    };
    let now = Utc::now();
    let out = rows
        .into_iter()
        .filter(|r| EquipmentKind::parse(&r.kind).is_some_and(|k| k.needs_battery()))
        .map(|r| {
            let last = match (&r.result, &r.checked_at) {
                (Some(res), Some(at)) => BatteryResult::parse(res)
                    .zip(parse_time(at))
                    .map(|(result, checked_at)| BatteryCheckRef { result, checked_at }),
                _ => None,
            };
            BatteryStatus {
                equipment_id: r.id,
                label: r.label,
                kind: r.kind,
                due: battery_due(last, &rule, now),
                last_result: r.result,
                last_checked_at: r.checked_at,
                last_checked_by: r.checked_by,
            }
        })
        .collect();
    Ok(Json(out))
}

// ---------------- alerts ----------------

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Alert {
    pub id: i64,
    pub show_id: Option<i64>,
    pub severity: String,
    pub source: String,
    pub equipment_id: Option<i64>,
    pub performer_id: Option<i64>,
    pub title: String,
    pub detail: String,
    pub dedupe_key: Option<String>,
    pub state: String,
    pub opened_at: String,
    pub opened_by: String,
    pub acked_at: Option<String>,
    pub acked_by: Option<String>,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
    pub resolution_note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NewAlert {
    pub show_id: Option<i64>,
    pub severity: String,
    pub equipment_id: Option<i64>,
    pub performer_id: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Deserialize)]
pub struct AlertFilter {
    /// "active" (open + acknowledged), "open", "acknowledged", "resolved", or omitted for all.
    state: Option<String>,
    show_id: Option<i64>,
}

async fn list_alerts(
    State(s): State<AppState>,
    Query(f): Query<AlertFilter>,
) -> ApiResult<Json<Vec<Alert>>> {
    let state = f.state.as_deref();
    if let Some(st) = state {
        if st != "active" && AlertState::parse(st).is_none() {
            return Err(ApiError::bad(
                "state must be active, open, acknowledged or resolved",
            ));
        }
    }
    Ok(Json(
        sqlx::query_as::<_, Alert>(
            "SELECT * FROM alerts
             WHERE (?1 IS NULL OR (?1 = 'active' AND state <> 'resolved') OR state = ?1)
               AND (?2 IS NULL OR show_id = ?2)
             ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END, opened_at DESC",
        )
        .bind(state)
        .bind(f.show_id)
        .fetch_all(&s.pool)
        .await?,
    ))
}

async fn create_alert(
    State(s): State<AppState>,
    actor: Actor,
    Json(n): Json<NewAlert>,
) -> ApiResult<(StatusCode, Json<Alert>)> {
    Severity::parse(&n.severity)
        .ok_or_else(|| ApiError::bad("severity must be info, warning or critical"))?;
    if n.title.trim().is_empty() {
        return Err(ApiError::bad("title is required"));
    }
    let mut tx = s.pool.begin().await?;
    let id = sqlx::query(
        "INSERT INTO alerts (show_id, severity, source, equipment_id, performer_id, title, detail, opened_at, opened_by)
         VALUES (?, ?, 'manual', ?, ?, ?, ?, ?, ?)",
    )
    .bind(n.show_id)
    .bind(&n.severity)
    .bind(n.equipment_id)
    .bind(n.performer_id)
    .bind(n.title.trim())
    .bind(&n.detail)
    .bind(now())
    .bind(&actor.name)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    let a = fetch_alert(&mut tx, id).await?;
    log_event(&mut tx, &actor, "alert.opened", &a).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(a)))
}

async fn fetch_alert(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, id: i64) -> ApiResult<Alert> {
    sqlx::query_as::<_, Alert>("SELECT * FROM alerts WHERE id = ?")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ApiError::NotFound)
}

async fn apply_alert_action(
    s: &AppState,
    actor: &Actor,
    id: i64,
    action: AlertAction,
    note: Option<String>,
) -> ApiResult<Alert> {
    let mut tx = s.pool.begin().await?;
    let a = fetch_alert(&mut tx, id).await?;
    let state =
        AlertState::parse(&a.state).ok_or_else(|| ApiError::bad("alert has unknown state"))?;
    let (new_state, implicit_ack) =
        alert::transition(state, action).map_err(|e| ApiError::Conflict(e.to_string()))?;
    let t = now();
    match action {
        AlertAction::Acknowledge => {
            sqlx::query("UPDATE alerts SET state = ?, acked_at = ?, acked_by = ? WHERE id = ?")
                .bind(new_state.as_str())
                .bind(&t)
                .bind(&actor.name)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        AlertAction::Resolve => {
            sqlx::query(
                "UPDATE alerts SET state = ?, resolved_at = ?, resolved_by = ?, resolution_note = ?,
                 acked_at = CASE WHEN ? THEN ? ELSE acked_at END,
                 acked_by = CASE WHEN ? THEN ? ELSE acked_by END
                 WHERE id = ?",
            )
            .bind(new_state.as_str())
            .bind(&t)
            .bind(&actor.name)
            .bind(&note)
            .bind(implicit_ack)
            .bind(&t)
            .bind(implicit_ack)
            .bind(&actor.name)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
    }
    let a = fetch_alert(&mut tx, id).await?;
    let kind = match action {
        AlertAction::Acknowledge => "alert.acknowledged",
        AlertAction::Resolve => "alert.resolved",
    };
    log_event(&mut tx, actor, kind, &a).await?;
    tx.commit().await?;
    Ok(a)
}

async fn ack_alert(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
) -> ApiResult<Json<Alert>> {
    Ok(Json(
        apply_alert_action(&s, &actor, id, AlertAction::Acknowledge, None).await?,
    ))
}

#[derive(Debug, Default, Deserialize)]
pub struct ResolveBody {
    pub note: Option<String>,
}

async fn resolve_alert(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    body: Option<Json<ResolveBody>>,
) -> ApiResult<Json<Alert>> {
    let note = body.and_then(|b| b.0.note);
    Ok(Json(
        apply_alert_action(&s, &actor, id, AlertAction::Resolve, note).await?,
    ))
}

// ---------------- events ----------------

#[derive(Debug, Serialize, FromRow)]
pub struct Event {
    pub seq: i64,
    pub at: String,
    pub actor: String,
    pub client_id: Option<String>,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub kind: String,
    #[sqlx(json)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct EventQuery {
    #[serde(default)]
    after: i64,
    limit: Option<i64>,
}

async fn list_events(
    State(s): State<AppState>,
    Query(q): Query<EventQuery>,
) -> ApiResult<Json<Vec<Event>>> {
    Ok(Json(
        sqlx::query_as::<_, Event>("SELECT * FROM event_log WHERE seq > ? ORDER BY seq LIMIT ?")
            .bind(q.after)
            .bind(q.limit.unwrap_or(500).clamp(1, 5000))
            .fetch_all(&s.pool)
            .await?,
    ))
}
