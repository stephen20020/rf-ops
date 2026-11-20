-- Phase 3: PIN sessions.

CREATE TABLE sessions (
    token       TEXT PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id   TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
