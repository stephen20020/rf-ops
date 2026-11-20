-- Phase 4: MIDI follow/lock per show, and key/value settings (bridge key, MIDI options).

ALTER TABLE shows ADD COLUMN midi_mode TEXT NOT NULL DEFAULT 'follow';

CREATE TABLE settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);
