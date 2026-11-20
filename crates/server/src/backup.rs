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

/// Automatic backups kept on the server's disk (R2 keeps 14 days; reliability §2.2).
pub const KEEP_AUTO: usize = 20;
/// Pinned backups older than this are removed from the server's disk (R2 keeps 90 days).
pub const PINNED_DAYS: i64 = 30;
/// Below this share of free space the health page warns.
pub const WARN_FREE_PCT: f64 = 25.0;
/// Below this share, local backups pause (a fresh one is still sent to R2, then deleted).
pub const CRITICAL_FREE_PCT: f64 = 10.0;
/// Never write a backup with less than this free, whatever the percentage.
pub const MIN_FREE_BYTES: u64 = 50 * 1024 * 1024;

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

/// Free space on the disk holding `dir`: bytes, percent, and "ok" / "warning" / "critical".
#[derive(Debug, Clone, Serialize)]
pub struct DiskState {
    pub free_bytes: Option<u64>,
    pub free_pct: Option<f64>,
    pub state: &'static str,
}

pub fn disk_state(dir: &Path) -> DiskState {
    let free = free_bytes(dir);
    let total = fs4::total_space(dir).ok().filter(|t| *t > 0);
    let pct = free.zip(total).map(|(f, t)| f as f64 * 100.0 / t as f64);
    DiskState {
        free_bytes: free,
        free_pct: pct.map(|p| (p * 10.0).round() / 10.0),
        state: state_for(pct, free),
    }
}

fn state_for(pct: Option<f64>, free: Option<u64>) -> &'static str {
    match (pct, free) {
        (_, Some(f)) if f < MIN_FREE_BYTES => "critical",
        (Some(p), _) if p < CRITICAL_FREE_PCT => "critical",
        (Some(p), _) if p < WARN_FREE_PCT => "warning",
        (Some(_), _) | (None, Some(_)) => "ok",
        (None, None) => "unknown",
    }
}

pub async fn run(pool: &SqlitePool, dir: &Path, reason: &str) -> Result<BackupFile, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    if let Some(free) = free_bytes(dir) {
        if free < MIN_FREE_BYTES {
            return Err(format!(
                "only {} MB free on the backup disk; backups stopped",
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

/// R2 key for a backup file: `<auto|pinned>/g<generation, 6 digits>/<name>`. The class comes
/// first so R2 lifecycle rules (`auto/` 14 days, `pinned/` 90 days) match. Restores take the
/// newest file of the highest generation, so a stale server's uploads are never chosen.
pub fn r2_key(generation: i64, name: &str) -> String {
    let class = if name.ends_with("-pin.db") { "pinned" } else { "auto" };
    format!("{class}/g{generation:06}/{name}")
}

/// Off-site uploader to R2. Refuses while fenced (a fenced server must not publish backups).
pub fn r2_offsite(r2: crate::r2::R2, lease: Option<crate::lease::LeaseHandle>) -> Offsite {
    Arc::new(move |path: PathBuf| {
        let r2 = r2.clone();
        let lease = lease.clone();
        Box::pin(async move {
            let generation = match &lease {
                Some(l) if l.is_fenced() => return Err("fenced: not uploading".to_string()),
                Some(l) => l.generation().max(1),
                None => 1,
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .ok_or("no file name")?;
            let body = tokio::fs::read(&path)
                .await
                .map_err(|e| format!("read {name}: {e}"))?;
            r2.put(&r2_key(generation, &name), body, crate::r2::Cond::None)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    })
}

/// The newest backup in R2: highest generation first, then the newest name (names embed a
/// sortable UTC timestamp). Returns the object key.
pub fn pick_newest(keys: &[String]) -> Option<String> {
    keys.iter()
        .filter_map(|k| {
            let mut parts = k.split('/');
            let class = parts.next()?;
            if class != "auto" && class != "pinned" {
                return None;
            }
            let g: i64 = parts.next()?.strip_prefix('g')?.parse().ok()?;
            let name = parts.next()?;
            (name.starts_with("rfops-") && name.ends_with(".db")).then(|| (g, name.to_string(), k.clone()))
        })
        .max_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)))
        .map(|(_, _, k)| k)
}

/// One backup plus its off-site copy, recording the outcome in `status`.
pub async fn backup_and_ship(
    pool: &SqlitePool,
    dir: &Path,
    reason: &str,
    status: &SharedStatus,
    offsite: Option<&Offsite>,
) -> Result<BackupFile, String> {
    let critical = disk_state(dir).state == "critical";
    if critical && offsite.is_none() {
        let e = "disk almost full (under 10 % free): local backups paused".to_string();
        status.lock().expect("backup status poisoned").last_error = Some(e.clone());
        return Err(e);
    }
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
        drop(st);
        if critical && !f.pinned {
            // Disk nearly full: the copy lives in R2 only.
            let _ = std::fs::remove_file(dir.join(&f.name));
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
    fn r2_keys_and_newest() {
        assert_eq!(r2_key(3, "rfops-20261120-100000.000-auto.db"), "auto/g000003/rfops-20261120-100000.000-auto.db");
        assert_eq!(r2_key(3, "rfops-x-show-start-pin.db"), "pinned/g000003/rfops-x-show-start-pin.db");
        let keys = vec![
            "auto/g000002/rfops-20261120-110000.000-auto.db".to_string(),
            "pinned/g000003/rfops-20261120-090000.000-show-start-pin.db".to_string(),
            "auto/g000003/rfops-20261120-100000.000-auto.db".to_string(),
            "auto/g000010/garbage.txt".to_string(),
            "lease.json".to_string(),
        ];
        // Highest generation wins even over a newer timestamp from an older generation.
        assert_eq!(pick_newest(&keys).unwrap(), "auto/g000003/rfops-20261120-100000.000-auto.db");
    }

    #[test]
    fn disk_thresholds() {
        let gb = 1024 * 1024 * 1024;
        assert_eq!(state_for(Some(50.0), Some(gb)), "ok");
        assert_eq!(state_for(Some(20.0), Some(gb)), "warning");
        assert_eq!(state_for(Some(9.0), Some(gb)), "critical");
        assert_eq!(state_for(Some(80.0), Some(10 * 1024 * 1024)), "critical");
    }

    #[test]
    fn pinned_reasons() {
        assert!(!is_pinned_reason("auto"));
        assert!(!is_pinned_reason("auto-auth"));
        assert!(is_pinned_reason("show-start"));
        assert!(is_pinned_reason("manual"));
    }
}
