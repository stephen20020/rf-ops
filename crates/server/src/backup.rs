//! Verified SQLite backups.
//!
//! Each backup is written with `VACUUM INTO` to a temporary file, checked with `quick_check`,
//! fsynced and only then renamed to its final name. A backup that fails is never listed.
//! Backups made at show start or by hand are *pinned*: automatic rotation never removes them.
//! While any show is in show mode, automatic backups run every 2 minutes; otherwise every 15.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use sqlx::SqlitePool;

/// Automatic backups kept (about 2 days at 15-minute intervals with no show running).
pub const KEEP_AUTO: usize = 200;
/// Pinned backups older than this are removed.
pub const PINNED_DAYS: i64 = 90;
/// Below this free space backups pause (and the health page reports critical).
pub const MIN_FREE_BYTES: u64 = 500 * 1024 * 1024;

pub const SHOW_INTERVAL: Duration = Duration::from_secs(120);
pub const IDLE_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub name: String,
    pub bytes: u64,
    pub created_at: String,
    pub pinned: bool,
    /// Only files that passed quick_check are ever renamed into place, so listed files are verified.
    pub verified: bool,
}

/// Status shared with the health page.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BackupStatus {
    pub last_ok_at: Option<String>,
    pub last_name: Option<String>,
    pub last_error: Option<String>,
    pub last_offsite_at: Option<String>,
    pub last_offsite_error: Option<String>,
}

pub type SharedStatus = Arc<Mutex<BackupStatus>>;

fn is_pinned_reason(reason: &str) -> bool {
    !reason.starts_with("auto")
}

/// Free bytes on the filesystem holding `dir`.
pub fn free_bytes(dir: &Path) -> Option<u64> {
    fs4::available_space(dir).ok()
}

pub async fn run(pool: &SqlitePool, dir: &Path, reason: &str) -> Result<BackupFile, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    if let Some(free) = free_bytes(dir) {
        if free < MIN_FREE_BYTES {
            return Err(format!(
                "only {} MB free on the backup disk; backups paused",
                free / 1024 / 1024
            ));
        }
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let safe_reason: String = reason
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let pin = if is_pinned_reason(reason) { "-pin" } else { "" };
    let final_path = dir.join(format!("rfops-{stamp}-{safe_reason}{pin}.db"));
    let tmp_path = dir.join(format!(".rfops-{stamp}.tmp"));
    let _ = std::fs::remove_file(&tmp_path);

    // The path is built here (never from user input) and inlined as an escaped SQL literal.
    // On sqlx's shared-cache in-memory databases VACUUM INTO writes nothing, so backups are only
    // enabled for file databases.
    let literal = tmp_path.to_string_lossy().replace('\'', "''");
    let result = async {
        sqlx::raw_sql(&format!("VACUUM INTO '{literal}'"))
            .execute(pool)
            .await
            .map_err(|e| format!("backup failed: {e}"))?;
        crate::db::quick_check_file(&tmp_path)
            .await
            .map_err(|e| format!("backup failed verification: {e}"))?;
        std::fs::File::open(&tmp_path)
            .and_then(|f| f.sync_all())
            .map_err(|e| format!("backup fsync failed: {e}"))?;
        std::fs::rename(&tmp_path, &final_path).map_err(|e| format!("backup rename failed: {e}"))
    }
    .await;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    // Keep the WAL small: checkpoint now that a consistent copy exists.
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await;
    rotate(dir);
    describe(&final_path).ok_or_else(|| "backup file missing after rename".to_string())
}

fn describe(p: &Path) -> Option<BackupFile> {
    let meta = std::fs::metadata(p).ok()?;
    let created: chrono::DateTime<chrono::Utc> = meta.modified().ok()?.into();
    let name = p.file_name()?.to_string_lossy().to_string();
    Some(BackupFile {
        pinned: name.ends_with("-pin.db"),
        verified: true,
        name,
        bytes: meta.len(),
        created_at: created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

/// Backups in `dir`, newest first. Temporary files from interrupted backups are never listed.
pub fn list(dir: &Path) -> Vec<BackupFile> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name().is_some_and(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("rfops-") && n.ends_with(".db")
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
    let cutoff = chrono::Utc::now() - chrono::Duration::days(PINNED_DAYS);
    for old in all.iter().filter(|f| !f.pinned).skip(KEEP_AUTO) {
        let _ = std::fs::remove_file(dir.join(&old.name));
    }
    for old in all.iter().filter(|f| f.pinned) {
        if chrono::DateTime::parse_from_rfc3339(&old.created_at).is_ok_and(|t| t < cutoff) {
            let _ = std::fs::remove_file(dir.join(&old.name));
        }
    }
    // Leftover temp files from interrupted runs.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(".rfops-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Upload hook for off-site copies (Cloudflare R2). Set by `with_offsite`.
pub type Offsite = Arc<
    dyn Fn(PathBuf) -> futures_util::future::BoxFuture<'static, Result<(), String>> + Send + Sync,
>;

/// One backup plus its off-site copy, recording the outcome in `status`.
pub async fn backup_and_ship(
    pool: &SqlitePool,
    dir: &Path,
    reason: &str,
    status: &SharedStatus,
    offsite: Option<&Offsite>,
) -> Result<BackupFile, String> {
    let r = run(pool, dir, reason).await;
    {
        let mut st = status.lock().expect("backup status poisoned");
        match &r {
            Ok(f) => {
                st.last_ok_at = Some(crate::db::now());
                st.last_name = Some(f.name.clone());
                st.last_error = None;
            }
            Err(e) => st.last_error = Some(e.clone()),
        }
    }
    if let (Ok(f), Some(up)) = (&r, offsite) {
        let res = up(dir.join(&f.name)).await;
        let mut st = status.lock().expect("backup status poisoned");
        match res {
            Ok(()) => {
                st.last_offsite_at = Some(crate::db::now());
                st.last_offsite_error = None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "off-site backup upload failed");
                st.last_offsite_error = Some(e);
            }
        }
    }
    r
}

/// Periodic backups: every 2 min while a show is in show mode, else every 15 min; skipped when
/// nothing changed since the last one.
pub async fn periodic(
    pool: SqlitePool,
    dir: PathBuf,
    status: SharedStatus,
    offsite: Option<Offsite>,
    show_every: Duration,
    idle_every: Duration,
) {
    let mut last_seq = -1i64;
    let mut waited = Duration::ZERO;
    let tick = Duration::from_secs(5).min(show_every);
    loop {
        tokio::time::sleep(tick).await;
        waited += tick;
        let show_mode: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM shows WHERE mode = 'show')")
                .fetch_one(&pool)
                .await
                .unwrap_or(false);
        let every = if show_mode { show_every } else { idle_every };
        if waited < every {
            continue;
        }
        waited = Duration::ZERO;
        let seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM event_log")
            .fetch_one(&pool)
            .await
            .unwrap_or(last_seq);
        if seq == last_seq {
            continue;
        }
        match backup_and_ship(&pool, &dir, "auto", &status, offsite.as_ref()).await {
            Ok(f) => {
                last_seq = seq;
                tracing::info!(file = %f.name, "backup written");
            }
            Err(e) => tracing::warn!(error = %e, "periodic backup failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_reasons() {
        assert!(!is_pinned_reason("auto"));
        assert!(is_pinned_reason("show-start"));
        assert!(is_pinned_reason("manual"));
    }
}
