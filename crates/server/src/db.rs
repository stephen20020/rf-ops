use std::str::FromStr;

use serde::Serialize;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Sqlite, SqlitePool, Transaction,
};

pub async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);
    // In-memory databases are per-connection, so tests use a single connection.
    let max = if url.contains(":memory:") { 1 } else { 8 };
    let pool = SqlitePoolOptions::new()
        .max_connections(max)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Who performed an action. Phase 1 takes this from request headers; PIN auth comes later.
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
