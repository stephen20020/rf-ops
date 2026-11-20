use std::{path::Path, str::FromStr, time::Duration};

use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Sqlite, SqlitePool, Transaction,
};

/// Open (and migrate) the database with durable settings:
/// - WAL with `synchronous=FULL` + `fullfsync`, so a committed swap survives power loss
///   (sqlite.org/pragma.html; macOS fsync does not flush drive caches without fullfsync);
/// - a 5 s busy timeout so backups and writers never fail with SQLITE_BUSY.
pub async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .busy_timeout(Duration::from_secs(5))
        .pragma("fullfsync", "ON")
        .pragma("checkpoint_fullfsync", "ON");
    // In-memory databases are per-connection, so tests use a single connection.
    let max = if url.contains(":memory:") { 1 } else { 8 };
    let pool = SqlitePoolOptions::new()
        .max_connections(max)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// `PRAGMA quick_check`: returns `Ok(())` when the database is intact, otherwise the problems.
pub async fn quick_check(pool: &SqlitePool) -> Result<(), String> {
    let rows: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(pool)
        .await
        .map_err(|e| format!("quick_check failed to run: {e}"))?;
    if rows.len() == 1 && rows[0] == "ok" {
        Ok(())
    } else {
        Err(rows.join("; "))
    }
}

/// Check a database file on disk without migrating it. Used for backups and before restores.
pub async fn quick_check_file(path: &Path) -> Result<(), String> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let r = quick_check(&pool).await;
    pool.close().await;
    r
}

/// Path of the database file for a `sqlite://` URL, or `None` for in-memory databases.
pub fn file_path(url: &str) -> Option<std::path::PathBuf> {
    let path = url
        .trim_start_matches("sqlite://")
        .trim_start_matches("sqlite:")
        .split('?')
        .next()
        .unwrap_or_default();
    (!path.is_empty() && !path.contains(":memory:")).then(|| std::path::PathBuf::from(path))
}

/// Open the database, verifying it first. If the file exists but fails `quick_check`, it is moved
/// aside (never deleted) and the newest verified backup in `backup_dir` is restored in its place.
/// Returns the pool and a note describing any recovery that happened.
pub async fn open_verified(
    url: &str,
    backup_dir: Option<&Path>,
) -> Result<(SqlitePool, Option<String>), String> {
    let Some(path) = file_path(url) else {
        return connect(url)
            .await
            .map(|p| (p, None))
            .map_err(|e| e.to_string());
    };
    let mut note = None;
    if path.exists() {
        if let Err(problem) = quick_check_file(&path).await {
            let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
            let aside = path.with_extension(format!("corrupt-{stamp}.db"));
            for suffix in ["", "-wal", "-shm"] {
                let from = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
                if from.exists() {
                    let to = std::path::PathBuf::from(format!("{}{suffix}", aside.display()));
                    std::fs::rename(&from, &to).map_err(|e| {
                        format!("database failed integrity check ({problem}) and could not be moved aside: {e}")
                    })?;
                }
            }
            let restored = backup_dir
                .and_then(|d| crate::backup::list(d).into_iter().find(|f| f.verified))
                .ok_or_else(|| {
                    format!(
                        "database failed integrity check ({problem}); moved to {} and no verified backup exists. Restore one by hand (docs/operations.md).",
                        aside.display()
                    )
                })?;
            let src = backup_dir.expect("checked above").join(&restored.name);
            std::fs::copy(&src, &path).map_err(|e| format!("restore copy failed: {e}"))?;
            note = Some(format!(
                "Database failed its integrity check ({problem}). The damaged file was kept as {} and backup {} was restored.",
                aside.display(),
                restored.name
            ));
        }
    }
    let pool = connect(url).await.map_err(|e| e.to_string())?;
    quick_check(&pool).await?;
    Ok((pool, note))
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Who performed an action.
#[derive(Debug, Clone)]
pub struct Actor {
    pub name: String,
    pub client_id: Option<String>,
    /// Role from the users table; `None` while no users are configured (bootstrap mode).
    pub role: Option<String>,
}

impl Actor {
    pub fn is_admin(&self) -> bool {
        self.role.as_deref().is_none_or(|r| r == "admin")
    }
}

/// Append to the event log inside the caller's transaction. Returns the new sequence number.
pub async fn log_event<T: Serialize>(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    kind: &str,
    payload: &T,
) -> Result<i64, sqlx::Error> {
    let payload = serde_json::to_string(payload).unwrap_or_else(|_| "null".into());
    let rec = sqlx::query(
        "INSERT INTO event_log (at, actor, client_id, type, payload) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(now())
    .bind(&actor.name)
    .bind(&actor.client_id)
    .bind(kind)
    .bind(payload)
    .execute(&mut **tx)
    .await?;
    Ok(rec.last_insert_rowid())
}
