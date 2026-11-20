//! Rule engine for alerts that are not raised by a person. Phase 3: battery checks in show mode.
//! Each rule condition has a dedupe key so it opens at most one active alert; rule alerts resolve
//! themselves when the condition clears.

use std::{collections::HashMap, sync::Arc, time::Duration};

use rfops_core::{assign, battery::BatteryDue};
use sqlx::SqlitePool;

use crate::{
    db::{log_event, now, Actor},
    hub::Hub,
    routes::{ops::compute_battery_status, shows::load_plan},
};

pub async fn run(pool: SqlitePool, hub: Arc<Hub>, every: Duration) {
    loop {
        if let Err(e) = evaluate(&pool).await {
            tracing::warn!(error = %e, "rule evaluation failed");
        } else {
            hub.wake();
        }
        tokio::time::sleep(every).await;
    }
}

fn system() -> Actor {
    Actor {
        name: "system".into(),
        client_id: None,
        role: None,
    }
}

/// Evaluate all rules once. Returns the number of alerts opened + resolved.
pub async fn evaluate(pool: &SqlitePool) -> Result<usize, crate::error::ApiError> {
    let shows: Vec<i64> = sqlx::query_scalar("SELECT id FROM shows WHERE mode = 'show'")
        .fetch_all(pool)
        .await?;
    let mut changed = 0;
    for show_id in shows {
        changed += battery_rule(pool, show_id).await?;
    }
    // Rule alerts for shows that left show mode are resolved too.
    let stale: Vec<(i64, String)> = sqlx::query_as(
        "SELECT a.id, a.title FROM alerts a JOIN shows s ON s.id = a.show_id
         WHERE a.source = 'rule' AND a.state <> 'resolved' AND s.mode <> 'show'",
    )
    .fetch_all(pool)
    .await?;
    for (id, _) in stale {
        resolve(pool, id, "Show left show mode").await?;
        changed += 1;
    }
    Ok(changed)
}

async fn battery_rule(pool: &SqlitePool, show_id: i64) -> Result<usize, crate::error::ApiError> {
    let plan = load_plan(pool, show_id).await?;
    let songs = plan.core_songs();
    let assignments = plan.core_assignments();
    let kits = plan.core_kits();

    // Carriers in use now or later in the show, and whether they are in use right now.
    let mut ordered: Vec<_> = songs.iter().collect();
    ordered.sort_by_key(|s| (s.position, s.id));
    let start = plan
        .show
        .current_song_id
        .and_then(|c| ordered.iter().position(|s| s.id == c))
        .unwrap_or(0);
    let mut in_use: HashMap<i64, bool> = HashMap::new();
    for (i, song) in ordered.iter().enumerate().skip(start) {
        let now_song = plan.show.current_song_id == Some(song.id) && i == start;
        for ks in assign::effective_assignment(&assignments, Some(song.id)).values() {
            for k in ks {
                if let Some(c) = kits.get(k).and_then(|k| k.carrier_id) {
                    let e = in_use.entry(c).or_insert(false);
                    *e |= now_song;
                }
            }
        }
    }

    let status = compute_battery_status(pool, show_id, None).await?;
    let mut changed = 0;
    for b in status {
        let key = format!("battery:{show_id}:{}", b.equipment_id);
        let active: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, severity FROM alerts WHERE dedupe_key = ? AND state <> 'resolved'",
        )
        .bind(&key)
        .fetch_optional(pool)
        .await?;
        let needed = in_use.get(&b.equipment_id).copied();
        let problem = match (b.due, needed) {
            (_, None) => None,
            (BatteryDue::Low, Some(now)) => {
                Some((if now { "critical" } else { "warning" }, "Battery LOW"))
            }
            (BatteryDue::Overdue, Some(_)) => Some(("warning", "Battery check overdue")),
            (BatteryDue::Unchecked, Some(_)) => Some(("warning", "Battery not checked")),
            (BatteryDue::Ok | BatteryDue::DueSoon, Some(_)) => None,
        };
        match (problem, active) {
            (Some((severity, what)), None) => {
                let mut tx = pool.begin().await?;
                let res = sqlx::query(
                    "INSERT OR IGNORE INTO alerts (show_id, severity, source, equipment_id, title, detail, dedupe_key, opened_at, opened_by)
                     VALUES (?, ?, 'rule', ?, ?, ?, ?, ?, 'system')",
                )
                .bind(show_id)
                .bind(severity)
                .bind(b.equipment_id)
                .bind(format!("{what}: {}", b.label))
                .bind("Raised automatically in show mode. Record a battery check to clear it.")
                .bind(&key)
                .bind(now())
                .execute(&mut *tx)
                .await?;
                if res.rows_affected() > 0 {
                    let id = res.last_insert_rowid();
                    log_event(
                        &mut tx,
                        &system(),
                        "alert.opened",
                        &serde_json::json!({ "id": id, "severity": severity, "title": format!("{what}: {}", b.label), "source": "rule", "equipment_id": b.equipment_id, "show_id": show_id }),
                    )
                    .await?;
                    changed += 1;
                }
                tx.commit().await?;
            }
            (Some((severity, _)), Some((id, current)))
                if severity == "critical" && current != "critical" =>
            {
                // Escalate: a low pack is now on stage.
                let mut tx = pool.begin().await?;
                sqlx::query("UPDATE alerts SET severity = 'critical', state = 'open', acked_at = NULL, acked_by = NULL WHERE id = ?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                log_event(
                    &mut tx,
                    &system(),
                    "alert.escalated",
                    &serde_json::json!({ "id": id, "severity": "critical" }),
                )
                .await?;
                tx.commit().await?;
                changed += 1;
            }
            (None, Some((id, _))) => {
                let source: String = sqlx::query_scalar("SELECT source FROM alerts WHERE id = ?")
                    .bind(id)
                    .fetch_one(pool)
                    .await?;
                if source == "rule" {
                    resolve(pool, id, "Battery checked").await?;
                    changed += 1;
                }
            }
            _ => {}
        }
    }
    Ok(changed)
}

async fn resolve(pool: &SqlitePool, id: i64, note: &str) -> Result<(), crate::error::ApiError> {
    let t = now();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE alerts SET state = 'resolved', resolved_at = ?, resolved_by = 'system', resolution_note = ?,
         acked_at = COALESCE(acked_at, ?), acked_by = COALESCE(acked_by, 'system') WHERE id = ?",
    )
    .bind(&t)
    .bind(note)
    .bind(&t)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    log_event(
        &mut tx,
        &system(),
        "alert.resolved",
        &serde_json::json!({ "id": id, "resolution_note": note }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
