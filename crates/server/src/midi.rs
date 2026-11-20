//! MIDI from MultiTracks Playback, via the RF Ops bridge running on the Playback Mac.
//!
//! The bridge posts each message to `/api/midi` with a shared key and sends a heartbeat every
//! two seconds. Messages are matched against the song triggers of the *target show*: the show in
//! show mode, otherwise the most recent rehearsal, otherwise the most recent show.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use rfops_core::midi::{decide, Decision, Dedup, MidiMsg, Trigger};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::{
    db::{log_event, now, Actor},
    error::ApiResult,
    hub::Hub,
};

/// A bridge counts as offline after this long without a heartbeat.
pub const OFFLINE_AFTER: Duration = Duration::from_secs(6);
/// In show mode, warn when no bridge has been heard for this long (also after server start).
pub const ALERT_AFTER: Duration = Duration::from_secs(10);
const RECENT: usize = 50;

#[derive(Debug, Clone, Serialize)]
pub struct BridgeInfo {
    pub name: String,
    pub port: Option<String>,
    pub ports: Vec<String>,
    pub online: bool,
    pub last_seen_at: String,
    #[serde(skip)]
    pub last_seen: Instant,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorEntry {
    pub at: String,
    pub source: String,
    pub message: String,
    pub outcome: String,
}

pub struct MidiState {
    started: Instant,
    dedup: Dedup,
    pub bridge: Option<BridgeInfo>,
    pub recent: VecDeque<MonitorEntry>,
}

impl MidiState {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            dedup: Dedup::default(),
            bridge: None,
            recent: VecDeque::new(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn record(&mut self, source: &str, message: String, outcome: &str) {
        self.recent.push_front(MonitorEntry {
            at: now(),
            source: source.to_owned(),
            message,
            outcome: outcome.to_owned(),
        });
        self.recent.truncate(RECENT);
    }
}

impl Default for MidiState {
    fn default() -> Self {
        Self::new()
    }
}

pub type Shared = Arc<Mutex<MidiState>>;

fn system(name: &str) -> Actor {
    Actor {
        name: name.to_owned(),
        client_id: None,
        role: None,
    }
}

pub async fn setting(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
}

pub async fn set_setting(
    pool: &SqlitePool,
    key: &str,
    value: Option<&str>,
) -> Result<(), sqlx::Error> {
    match value {
        Some(v) => {
            sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                .bind(key)
                .bind(v)
                .execute(pool)
                .await?;
        }
        None => {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(key)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

pub fn new_key() -> String {
    use rand::RngCore;
    let mut b = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Make sure a bridge key exists; returns it.
pub async fn ensure_key(pool: &SqlitePool) -> Result<String, sqlx::Error> {
    if let Some(k) = setting(pool, "midi.bridge_key").await? {
        return Ok(k);
    }
    let k = new_key();
    set_setting(pool, "midi.bridge_key", Some(&k)).await?;
    Ok(k)
}

/// The show MIDI applies to.
pub async fn target_show(
    pool: &SqlitePool,
) -> Result<Option<(i64, Option<i64>, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, current_song_id, midi_mode FROM shows
         ORDER BY (mode = 'show') DESC, (mode = 'rehearsal') DESC, id DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
}

/// Heartbeat (or any message) from a bridge. Logs an event when it comes online.
pub async fn heartbeat(
    pool: &SqlitePool,
    state: &Shared,
    hub: &Hub,
    name: &str,
    port: Option<String>,
    ports: Option<Vec<String>>,
) -> ApiResult<()> {
    let came_online = {
        let mut st = state.lock().expect("midi state poisoned");
        let was_online = st.bridge.as_ref().is_some_and(|b| b.online);
        let prev_ports = st
            .bridge
            .as_ref()
            .map(|b| b.ports.clone())
            .unwrap_or_default();
        st.bridge = Some(BridgeInfo {
            name: name.to_owned(),
            port: port.clone(),
            ports: ports.unwrap_or(prev_ports),
            online: true,
            last_seen_at: now(),
            last_seen: Instant::now(),
        });
        !was_online
    };
    if came_online {
        let mut tx = pool.begin().await?;
        log_event(
            &mut tx,
            &system(name),
            "midi.bridge_online",
            &serde_json::json!({ "bridge": name, "port": port }),
        )
        .await?;
        tx.commit().await?;
        hub.wake();
    }
    Ok(())
}

/// Process one MIDI message. Returns a short outcome for the monitor.
pub async fn process(
    pool: &SqlitePool,
    state: &Shared,
    hub: &Hub,
    msg: MidiMsg,
    source: &str,
) -> ApiResult<String> {
    let accepted = {
        let mut st = state.lock().expect("midi state poisoned");
        let t = st.now_ms();
        st.dedup.accept(msg, t)
    };
    let outcome = if !accepted {
        "duplicate — ignored".to_owned()
    } else {
        apply(pool, msg, source).await?
    };
    state
        .lock()
        .expect("midi state poisoned")
        .record(source, msg.describe(), &outcome);
    hub.wake();
    Ok(outcome)
}

async fn apply(pool: &SqlitePool, msg: MidiMsg, source: &str) -> ApiResult<String> {
    let Some((show_id, current, mode)) = target_show(pool).await? else {
        return Ok("no show to follow".into());
    };
    let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, title, midi_trigger FROM songs WHERE show_id = ? AND skipped = 0 ORDER BY position, id",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    let songs: Vec<(i64, Trigger)> = rows
        .iter()
        .filter_map(|(id, _, t)| Some((*id, Trigger::parse_json(t.as_deref()?)?)))
        .collect();
    let next = setting(pool, "midi.next")
        .await?
        .and_then(|s| Trigger::parse_json(&s));
    let prev = setting(pool, "midi.prev")
        .await?
        .and_then(|s| Trigger::parse_json(&s));
    let decision = decide(&msg, &songs, next, prev);
    let title = |id: i64| {
        rows.iter()
            .find(|r| r.0 == id)
            .map(|r| r.1.clone())
            .unwrap_or_default()
    };

    let target = match &decision {
        Decision::GoToSong { song_id } => Some(*song_id),
        Decision::Next | Decision::Prev => {
            let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
            let idx = current.and_then(|c| ids.iter().position(|&i| i == c));
            match (&decision, idx) {
                (Decision::Next, None) => ids.first().copied(),
                (Decision::Next, Some(i)) => ids.get(i + 1).copied(),
                (Decision::Prev, Some(i)) if i > 0 => ids.get(i - 1).copied(),
                _ => None,
            }
        }
        Decision::Unmatched => {
            let key = format!("midi-unmatched:{show_id}:{}", msg.describe());
            let title = format!("MIDI {} matches no song", msg.describe());
            let mut tx = pool.begin().await?;
            let r = sqlx::query(
                "INSERT OR IGNORE INTO alerts (show_id, severity, source, title, detail, dedupe_key, opened_at, opened_by)
                 VALUES (?, 'warning', 'midi', ?, ?, ?, ?, 'midi')",
            )
            .bind(show_id)
            .bind(&title)
            .bind("Playback sent a cue that no song in this show listens for. Check the song's MIDI setting (Admin → Show & kits) or the Playback cue. The current song was not changed.")
            .bind(&key)
            .bind(now())
            .execute(&mut *tx)
            .await?;
            if r.rows_affected() > 0 {
                log_event(
                    &mut tx,
                    &system("midi"),
                    "alert.opened",
                    &serde_json::json!({ "id": r.last_insert_rowid(), "severity": "warning", "title": title, "show_id": show_id }),
                )
                .await?;
            }
            tx.commit().await?;
            return Ok("no matching song — warning raised".into());
        }
        Decision::Ignore => return Ok("not a song cue — ignored".into()),
    };

    let Some(target) = target else {
        return Ok("already at the end of the setlist".into());
    };
    if mode == "lock" {
        let mut tx = pool.begin().await?;
        log_event(
            &mut tx,
            &system("midi"),
            "midi.ignored_locked",
            &serde_json::json!({ "show_id": show_id, "song_id": target, "midi": msg.describe() }),
        )
        .await?;
        tx.commit().await?;
        return Ok(format!("LOCKED — would go to {}", title(target)));
    }
    if Some(target) == current {
        return Ok(format!("already on {}", title(target)));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE shows SET current_song_id = ? WHERE id = ?")
        .bind(target)
        .bind(show_id)
        .execute(&mut *tx)
        .await?;
    log_event(
        &mut tx,
        &system(source),
        "show.current_song",
        &serde_json::json!({ "show_id": show_id, "song_id": target, "from_song_id": current, "source": "midi", "midi": msg.describe() }),
    )
    .await?;
    tx.commit().await?;
    Ok(format!("→ {}", title(target)))
}

/// Marks the bridge offline and raises/clears the show-mode "bridge offline" alert.
pub async fn watchdog(pool: SqlitePool, state: Shared, hub: Arc<Hub>) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Err(e) = watch_once(&pool, &state, &hub).await {
            tracing::warn!(error = %e, "midi watchdog failed");
        }
    }
}

pub async fn watch_once(pool: &SqlitePool, state: &Shared, hub: &Hub) -> ApiResult<()> {
    let (went_offline, silent_for, name) = {
        let mut st = state.lock().expect("midi state poisoned");
        let started = st.started;
        match st.bridge.as_mut() {
            Some(b) => {
                let gone = b.online && b.last_seen.elapsed() > OFFLINE_AFTER;
                if gone {
                    b.online = false;
                }
                (
                    gone,
                    if b.online {
                        Duration::ZERO
                    } else {
                        b.last_seen.elapsed()
                    },
                    b.name.clone(),
                )
            }
            None => (false, started.elapsed(), "bridge".to_owned()),
        }
    };
    if went_offline {
        let mut tx = pool.begin().await?;
        log_event(
            &mut tx,
            &system("midi"),
            "midi.bridge_offline",
            &serde_json::json!({ "bridge": name }),
        )
        .await?;
        tx.commit().await?;
        hub.wake();
    }

    // Show-mode alert, only if the show actually uses MIDI cues.
    let show: Option<i64> = sqlx::query_scalar(
        "SELECT s.id FROM shows s WHERE s.mode = 'show'
         AND EXISTS (SELECT 1 FROM songs WHERE show_id = s.id AND midi_trigger IS NOT NULL) LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    let active: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM alerts WHERE dedupe_key LIKE 'midi-bridge:%' AND state <> 'resolved' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    match (show, silent_for > ALERT_AFTER, active) {
        (Some(show_id), true, None) => {
            let mut tx = pool.begin().await?;
            let r = sqlx::query(
                "INSERT OR IGNORE INTO alerts (show_id, severity, source, title, detail, dedupe_key, opened_at, opened_by)
                 VALUES (?, 'warning', 'rule', 'MIDI bridge offline', ?, ?, ?, 'system')",
            )
            .bind(show_id)
            .bind("Song changes from Playback are not arriving. Advance songs by hand, and check the RF Ops bridge on the Playback Mac.")
            .bind(format!("midi-bridge:{show_id}"))
            .bind(now())
            .execute(&mut *tx)
            .await?;
            if r.rows_affected() > 0 {
                log_event(
                    &mut tx,
                    &system("system"),
                    "alert.opened",
                    &serde_json::json!({ "id": r.last_insert_rowid(), "severity": "warning", "title": "MIDI bridge offline", "show_id": show_id }),
                )
                .await?;
            }
            tx.commit().await?;
            hub.wake();
        }
        (_, false, Some(id)) | (None, _, Some(id)) => {
            let t = now();
            let mut tx = pool.begin().await?;
            sqlx::query(
                "UPDATE alerts SET state = 'resolved', resolved_at = ?, resolved_by = 'system', resolution_note = 'MIDI bridge back online',
                 acked_at = COALESCE(acked_at, ?), acked_by = COALESCE(acked_by, 'system') WHERE id = ?",
            )
            .bind(&t)
            .bind(&t)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            log_event(
                &mut tx,
                &system("system"),
                "alert.resolved",
                &serde_json::json!({ "id": id }),
            )
            .await?;
            tx.commit().await?;
            hub.wake();
        }
        _ => {}
    }
    Ok(())
}
