//! Real-time sync: event broadcaster, WebSocket clients, and request middleware.
//!
//! Every mutation commits an `event_log` row in its own transaction. After any successful
//! non-GET request, the broadcaster is woken, reads new rows from the log in `seq` order and
//! fans them out to WebSocket clients. Clients reconnect with `?after=<last seq>` and get the
//! missed events replayed (or a `resync` message if they are too far behind).

use std::{sync::Arc, time::Duration};

use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, Request, State,
    },
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tokio::sync::{broadcast, Notify};

use crate::{db::now, AppState, IDEMPOTENCY_HEADER};

#[derive(Debug, Clone, Serialize, FromRow)]
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

/// Max events replayed to a reconnecting client before it is told to resync instead.
pub const REPLAY_LIMIT: i64 = 1000;

pub struct Hub {
    notify: Notify,
    tx: broadcast::Sender<Arc<Event>>,
}

impl Hub {
    pub async fn start(pool: SqlitePool) -> Result<Arc<Self>, sqlx::Error> {
        let (tx, _) = broadcast::channel(1024);
        let hub = Arc::new(Hub {
            notify: Notify::new(),
            tx,
        });
        let last = max_seq(&pool).await?;
        tokio::spawn(broadcaster(pool, hub.clone(), last));
        Ok(hub)
    }

    pub fn wake(&self) {
        self.notify.notify_one();
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.tx.subscribe()
    }
}

pub async fn max_seq(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM event_log")
        .fetch_one(pool)
        .await
}

pub async fn events_after(
    pool: &SqlitePool,
    after: i64,
    limit: i64,
) -> Result<Vec<Event>, sqlx::Error> {
    sqlx::query_as::<_, Event>("SELECT * FROM event_log WHERE seq > ? ORDER BY seq LIMIT ?")
        .bind(after)
        .bind(limit)
        .fetch_all(pool)
        .await
}

async fn broadcaster(pool: SqlitePool, hub: Arc<Hub>, mut last: i64) {
    loop {
        // Woken by writes; the timer is a safety net for anything that bypasses the middleware.
        tokio::select! {
            _ = hub.notify.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
        loop {
            match events_after(&pool, last, 500).await {
                Ok(batch) if batch.is_empty() => break,
                Ok(batch) => {
                    for e in batch {
                        last = e.seq;
                        // No receivers is fine.
                        let _ = hub.tx.send(Arc::new(e));
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "broadcaster read failed");
                    break;
                }
            }
        }
    }
}

/// Wake the broadcaster after successful writes.
pub async fn notify_on_write(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let write = req.method() != Method::GET && req.method() != Method::HEAD;
    let res = next.run(req).await;
    if write && res.status().is_success() {
        s.hub.wake();
    }
    res
}

/// Replays the stored response when a write is retried with the same idempotency key, so a
/// client resending after a dropped connection never applies a command twice.
pub async fn idempotency(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let key = req
        .headers()
        .get(IDEMPOTENCY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let Some(key) = key.filter(|_| req.method() != Method::GET) else {
        return next.run(req).await;
    };
    if key.len() > 128 {
        return (StatusCode::BAD_REQUEST, "idempotency key too long").into_response();
    }

    let stored: Result<Option<(i64, Vec<u8>)>, _> =
        sqlx::query_as("SELECT status, body FROM idempotency WHERE key = ?")
            .bind(&key)
            .fetch_optional(&s.pool)
            .await;
    if let Ok(Some((status, body))) = stored {
        return replay(status, body);
    }

    let res = next.run(req).await;
    let (parts, body) = res.into_parts();
    let bytes = match to_bytes(body, 4 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    // Only remember definitive outcomes; server errors may be retried for real.
    if !parts.status.is_server_error() {
        let _ = sqlx::query(
            "INSERT OR IGNORE INTO idempotency (key, status, body, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&key)
        .bind(parts.status.as_u16() as i64)
        .bind(bytes.to_vec())
        .bind(now())
        .execute(&s.pool)
        .await;
    }
    Response::from_parts(parts, Body::from(bytes))
}

fn replay(status: i64, body: Vec<u8>) -> Response {
    let status = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK);
    let mut res = Response::new(Body::from(body));
    *res.status_mut() = status;
    res.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("application/json"),
    );
    res.headers_mut().insert(
        "x-rfops-replayed",
        axum::http::HeaderValue::from_static("1"),
    );
    res
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    after: Option<i64>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(s): State<AppState>,
    Query(q): Query<WsQuery>,
) -> Response {
    ws.on_upgrade(move |socket| client(socket, s, q.after))
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Outgoing<'a> {
    Hello {
        seq: i64,
        server_time: String,
    },
    Event {
        event: &'a Event,
    },
    /// The client missed too much; it must reload its snapshot.
    Resync {
        seq: i64,
    },
}

async fn send(socket: &mut WebSocket, msg: &Outgoing<'_>) -> bool {
    let text = serde_json::to_string(msg).unwrap_or_default();
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn client(mut socket: WebSocket, s: AppState, after: Option<i64>) {
    // Subscribe before reading the log so nothing falls between replay and live events.
    let mut rx = s.hub.subscribe();
    let Ok(head) = max_seq(&s.pool).await else {
        return;
    };
    if !send(
        &mut socket,
        &Outgoing::Hello {
            seq: head,
            server_time: now(),
        },
    )
    .await
    {
        return;
    }

    let mut last = head;
    if let Some(after) = after.filter(|a| *a < head) {
        match events_after(&s.pool, after, REPLAY_LIMIT).await {
            Ok(missed) if (missed.len() as i64) < REPLAY_LIMIT => {
                for e in &missed {
                    if !send(&mut socket, &Outgoing::Event { event: e }).await {
                        return;
                    }
                }
                last = missed.last().map(|e| e.seq).unwrap_or(after);
            }
            _ => {
                if !send(&mut socket, &Outgoing::Resync { seq: head }).await {
                    return;
                }
            }
        }
    }

    let mut ping = tokio::time::interval(Duration::from_secs(15));
    ping.tick().await;
    loop {
        tokio::select! {
            r = rx.recv() => match r {
                Ok(e) => {
                    if e.seq > last {
                        if !send(&mut socket, &Outgoing::Event { event: &e }).await { return; }
                        last = e.seq;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    let head = max_seq(&s.pool).await.unwrap_or(last);
                    if !send(&mut socket, &Outgoing::Resync { seq: head }).await { return; }
                    last = head;
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            m = socket.recv() => match m {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() { return; }
            }
        }
    }
}
