-- RF Ops initial schema

CREATE TABLE equipment (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,
    make        TEXT NOT NULL DEFAULT '',
    model       TEXT NOT NULL DEFAULT '',
    label       TEXT NOT NULL,
    serial      TEXT,
    parent_id   INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    channel_no  INTEGER,
    ip_address  TEXT,
    status      TEXT NOT NULL DEFAULT 'available',
    notes       TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX equipment_label ON equipment(label);

CREATE TABLE users (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    pin_hash  TEXT,
    role      TEXT NOT NULL DEFAULT 'a2'
);

CREATE TABLE performers (
    id                INTEGER PRIMARY KEY,
    name              TEXT NOT NULL,
    role              TEXT NOT NULL DEFAULT '',
    photo_path        TEXT,
    notes             TEXT NOT NULL DEFAULT '',
    owner_user_id     INTEGER REFERENCES users(id) ON DELETE SET NULL,
    zone              TEXT
);

CREATE TABLE shows (
    id               INTEGER PRIMARY KEY,
    name             TEXT NOT NULL,
    starts_at        TEXT,
    call_time        TEXT,
    current_song_id  INTEGER,
    mode             TEXT NOT NULL DEFAULT 'setup'
);

CREATE TABLE songs (
    id            INTEGER PRIMARY KEY,
    show_id       INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    position      INTEGER NOT NULL,
    title         TEXT NOT NULL,
    midi_trigger  TEXT,
    skipped       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX songs_show ON songs(show_id, position);

CREATE TABLE kits (
    id                    INTEGER PRIMARY KEY,
    show_id               INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    kind                  TEXT NOT NULL,
    name                  TEXT NOT NULL,
    base_id               INTEGER REFERENCES equipment(id),
    carrier_id            INTEGER REFERENCES equipment(id),
    element_id            INTEGER REFERENCES equipment(id),
    console_channel       INTEGER,
    console_channel_name  TEXT,
    dca                   TEXT,
    iem_mix               TEXT,
    is_spare              INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX kits_show ON kits(show_id);

CREATE TABLE assignments (
    id            INTEGER PRIMARY KEY,
    show_id       INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    performer_id  INTEGER NOT NULL REFERENCES performers(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL,
    kit_id        INTEGER REFERENCES kits(id) ON DELETE CASCADE,
    song_id       INTEGER REFERENCES songs(id) ON DELETE CASCADE
);
CREATE INDEX assignments_show ON assignments(show_id);

CREATE TABLE checkouts (
    id            INTEGER PRIMARY KEY,
    equipment_id  INTEGER NOT NULL REFERENCES equipment(id),
    performer_id  INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    show_id       INTEGER REFERENCES shows(id) ON DELETE SET NULL,
    out_at        TEXT NOT NULL,
    out_by        TEXT NOT NULL,
    in_at         TEXT,
    in_by         TEXT
);
CREATE INDEX checkouts_open ON checkouts(equipment_id) WHERE in_at IS NULL;

CREATE TABLE battery_checks (
    id            INTEGER PRIMARY KEY,
    equipment_id  INTEGER NOT NULL REFERENCES equipment(id) ON DELETE CASCADE,
    result        TEXT NOT NULL,
    battery_type  TEXT NOT NULL DEFAULT '',
    battery_id    INTEGER REFERENCES equipment(id),
    checked_at    TEXT NOT NULL,
    checked_by    TEXT NOT NULL,
    source        TEXT NOT NULL DEFAULT 'manual',
    notes         TEXT NOT NULL DEFAULT ''
);
CREATE INDEX battery_checks_eq ON battery_checks(equipment_id, checked_at);

CREATE TABLE alerts (
    id               INTEGER PRIMARY KEY,
    show_id          INTEGER REFERENCES shows(id) ON DELETE SET NULL,
    severity         TEXT NOT NULL,
    source           TEXT NOT NULL,
    equipment_id     INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    performer_id     INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    title            TEXT NOT NULL,
    detail           TEXT NOT NULL DEFAULT '',
    dedupe_key       TEXT,
    state            TEXT NOT NULL DEFAULT 'open',
    opened_at        TEXT NOT NULL,
    opened_by        TEXT NOT NULL,
    acked_at         TEXT,
    acked_by         TEXT,
    resolved_at      TEXT,
    resolved_by      TEXT,
    resolution_note  TEXT
);
CREATE UNIQUE INDEX alerts_dedupe_active ON alerts(dedupe_key) WHERE dedupe_key IS NOT NULL AND state <> 'resolved';

CREATE TABLE swaps (
    id              INTEGER PRIMARY KEY,
    show_id         INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    performer_id    INTEGER NOT NULL REFERENCES performers(id),
    from_kit_id     INTEGER REFERENCES kits(id),
    to_kit_id       INTEGER NOT NULL REFERENCES kits(id),
    scope           TEXT NOT NULL,
    song_id         INTEGER REFERENCES songs(id),
    reason          TEXT NOT NULL DEFAULT '',
    alert_id        INTEGER REFERENCES alerts(id),
    at              TEXT NOT NULL,
    by              TEXT NOT NULL
);

CREATE TABLE event_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    at         TEXT NOT NULL,
    actor      TEXT NOT NULL,
    client_id  TEXT,
    type       TEXT NOT NULL,
    payload    TEXT NOT NULL
);
