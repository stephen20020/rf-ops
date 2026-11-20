-- Step 1b: cloud sign-in. Sessions become signed stateless cookies; these tables hold what
-- must persist: revocations, enrolled screens and helper join codes.

ALTER TABLE users ADD COLUMN sessions_valid_after TEXT;
ALTER TABLE users ADD COLUMN zone TEXT;

CREATE TABLE session_revocations (
    sid         TEXT PRIMARY KEY,
    expires_at  INTEGER NOT NULL
);

CREATE TABLE screens (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    role          TEXT NOT NULL DEFAULT 'a2',
    enrolled_by   TEXT NOT NULL,
    enrolled_at   TEXT NOT NULL,
    last_seen_at  TEXT,
    revoked       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE join_codes (
    code_hash   TEXT PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    uses_left   INTEGER NOT NULL DEFAULT 3,
    expires_at  INTEGER NOT NULL,
    created_by  TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

-- PIN sessions from 0003 are replaced by signed cookies.
DELETE FROM sessions;
