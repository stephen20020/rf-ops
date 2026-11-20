-- v3.1 data model (docs/data-model.md): devices shared by every show, a device × song plot,
-- passes per visit, substitutions, physical holdings. Added alongside the old kit tables, which
-- the current screens still use until they move over.

CREATE TABLE events (
    id        INTEGER PRIMARY KEY,
    name      TEXT NOT NULL UNIQUE,
    timezone  TEXT NOT NULL DEFAULT 'America/Chicago'
);

ALTER TABLE shows ADD COLUMN event_id INTEGER REFERENCES events(id) ON DELETE SET NULL;
ALTER TABLE shows ADD COLUMN day TEXT;
ALTER TABLE shows ADD COLUMN set_name TEXT;
ALTER TABLE shows ADD COLUMN follows_show_id INTEGER REFERENCES shows(id) ON DELETE SET NULL;
ALTER TABLE shows ADD COLUMN activated_at TEXT;
ALTER TABLE shows ADD COLUMN ended_at TEXT;
ALTER TABLE songs ADD COLUMN notes TEXT NOT NULL DEFAULT '';
ALTER TABLE performers ADD COLUMN absent_note TEXT;

CREATE TABLE devices (
    id                 INTEGER PRIMARY KEY,
    label              TEXT NOT NULL UNIQUE,
    kind               TEXT NOT NULL CHECK (kind IN ('mic', 'iem', 'guitar')),
    carrier_id         INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    base_id            INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    element_id         INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    console_channel    TEXT,
    is_spare           INTEGER NOT NULL DEFAULT 0,
    guest_owner_id     INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    board_order        INTEGER NOT NULL DEFAULT 0,
    active             INTEGER NOT NULL DEFAULT 1,
    -- ok | suspect | out | missing (set by A2s; equipment 'faulty' also counts as out)
    status             TEXT NOT NULL DEFAULT 'ok'
);

-- IEM transmitter → mix, per show.
CREATE TABLE show_bases (
    show_id   INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    base_id   INTEGER NOT NULL REFERENCES equipment(id) ON DELETE CASCADE,
    mix       TEXT NOT NULL,
    PRIMARY KEY (show_id, base_id)
);

-- song_key 0 = the Default column (SQLite treats NULLs as distinct in a primary key).
CREATE TABLE plot_cells (
    show_id       INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    device_id     INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    song_key      INTEGER NOT NULL DEFAULT 0,
    performer_id  INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    element_id    INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    PRIMARY KEY (show_id, device_id, song_key)
);

CREATE TABLE passes (
    id               INTEGER PRIMARY KEY,
    show_id          INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    device_id        INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    song_id          INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    seq              INTEGER NOT NULL,
    to_performer_id  INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    UNIQUE (device_id, song_id, seq)
);

CREATE TABLE song_visits (
    id          INTEGER PRIMARY KEY,
    show_id     INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    visit_no    INTEGER NOT NULL,
    song_id     INTEGER NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    started_at  TEXT NOT NULL,
    via         TEXT NOT NULL,
    UNIQUE (show_id, visit_no)
);

CREATE TABLE pass_marks (
    pass_id   INTEGER NOT NULL REFERENCES passes(id) ON DELETE CASCADE,
    visit_no  INTEGER NOT NULL,
    by_name   TEXT NOT NULL,
    at        TEXT NOT NULL,
    PRIMARY KEY (pass_id, visit_no)
);

CREATE TABLE substitutions (
    id                   INTEGER PRIMARY KEY,
    show_id              INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    kind                 TEXT NOT NULL,
    performer_id         INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    failed_id            INTEGER REFERENCES devices(id) ON DELETE SET NULL,
    replacement_id       INTEGER REFERENCES devices(id) ON DELETE SET NULL,
    failed_base_id       INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    replacement_base_id  INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    retune               INTEGER NOT NULL DEFAULT 0,
    range                TEXT NOT NULL,
    state                TEXT NOT NULL,
    from_visit           INTEGER NOT NULL,
    starts_at_song_id    INTEGER REFERENCES songs(id) ON DELETE SET NULL,
    ended_at_visit       INTEGER,
    failed_component     TEXT NOT NULL DEFAULT 'unknown',
    created_by           TEXT NOT NULL,
    created_at           TEXT NOT NULL
);

-- Physical truth: who has each device now.
CREATE TABLE holdings (
    device_id      INTEGER PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
    holder_kind    TEXT NOT NULL DEFAULT 'stock' CHECK (holder_kind IN ('stock', 'person', 'owner')),
    performer_id   INTEGER REFERENCES performers(id) ON DELETE SET NULL,
    element_id     INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    tuned_base_id  INTEGER REFERENCES equipment(id) ON DELETE SET NULL,
    since          TEXT NOT NULL,
    by_name        TEXT NOT NULL
);

-- "Leave it" on an overdue handoff, for one visit.
CREATE TABLE handoff_left (
    show_id   INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    visit_no  INTEGER NOT NULL,
    device_id INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    PRIMARY KEY (show_id, visit_no, device_id)
);

CREATE TABLE performer_absences (
    show_id       INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    performer_id  INTEGER NOT NULL REFERENCES performers(id) ON DELETE CASCADE,
    PRIMARY KEY (show_id, performer_id)
);
