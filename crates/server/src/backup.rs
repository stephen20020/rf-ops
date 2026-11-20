//! SQLite backups via `VACUUM INTO`: every 15 minutes, on entering show mode, and on demand.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use sqlx::SqlitePool;

/// Number of backup files kept; older ones are deleted.
pub const KEEP: usize = 40;

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub name: String,
    pub bytes: u64,
    pub created_at: String,
}

pub async fn run(pool: &SqlitePool, dir: &Path, reason: &str) -> Result<BackupFile, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let safe_reason: String = reason
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = dir.join(format!("rfops-{stamp}-{safe_reason}.db"));
    // The path is built here (never from user input) and inlined as an escaped SQL literal.
    // Note: on sqlx's shared-cache in-memory databases VACUUM INTO writes nothing, so backups are
    // only enabled for file databases.
    let literal = path.to_string_lossy().replace('\'', "''");
    sqlx::raw_sql(&format!("VACUUM INTO '{literal}'"))
        .execute(pool)
        .await
        .map_err(|e| format!("backup failed: {e}"))?;
    rotate(dir);
    describe(&path).ok_or_else(|| "backup file missing after VACUUM INTO".to_string())
}

fn describe(p: &Path) -> Option<BackupFile> {
    let meta = std::fs::metadata(p).ok()?;
    let created: chrono::DateTime<chrono::Utc> = meta.modified().ok()?.into();
    Some(BackupFile {
        name: p.file_name()?.to_string_lossy().into(),
        bytes: meta.len(),
        created_at: created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

/// Backups in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<BackupFile> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name().is_some_and(|n| {
                        n.to_string_lossy().starts_with("rfops-")
                            && n.to_string_lossy().ends_with(".db")
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // Names embed a sortable UTC timestamp.
    files.sort();
    files.reverse();
    files.iter().filter_map(|p| describe(p)).collect()
}

fn rotate(dir: &Path) {
    let all = list(dir);
    for old in all.iter().skip(KEEP) {
        let _ = std::fs::remove_file(dir.join(&old.name));
    }
}

/// Periodic backups. Skips when nothing changed since the last one.
pub async fn periodic(pool: SqlitePool, dir: PathBuf, every: Duration) {
    let mut last_seq = -1i64;
    loop {
        tokio::time::sleep(every).await;
        let seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM event_log")
            .fetch_one(&pool)
            .await
            .unwrap_or(last_seq);
        if seq == last_seq {
            continue;
        }
        match run(&pool, &dir, "auto").await {
            Ok(f) => {
                last_seq = seq;
                tracing::info!(file = %f.name, "backup written");
            }
            Err(e) => tracing::warn!(error = %e, "periodic backup failed"),
        }
    }
}
