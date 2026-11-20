-- Phase 2: handoff completion tracking and idempotent command retries.

CREATE TABLE handoff_done (
    id            INTEGER PRIMARY KEY,
    show_id       INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    -- -1 means "before show" / "after show" so the unique index works.
    from_song     INTEGER NOT NULL,
    to_song       INTEGER NOT NULL,
    item_id       INTEGER NOT NULL,
    handoff_type  TEXT NOT NULL,
    done_by       TEXT NOT NULL,
    done_at       TEXT NOT NULL
);
CREATE UNIQUE INDEX handoff_done_key ON handoff_done(show_id, from_song, to_song, item_id, handoff_type);

CREATE TABLE idempotency (
    key         TEXT PRIMARY KEY,
    status      INTEGER NOT NULL,
    body        BLOB NOT NULL,
    created_at  TEXT NOT NULL
);
