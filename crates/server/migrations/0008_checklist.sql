-- Pre-show checklist (interface spec §12.1): ticks for manual items, and extra items per show.
CREATE TABLE checklist_ticks (
    show_id    INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    item_key   TEXT NOT NULL,
    ticked_by  TEXT NOT NULL,
    ticked_at  TEXT NOT NULL,
    PRIMARY KEY (show_id, item_key)
);

CREATE TABLE checklist_custom (
    id       INTEGER PRIMARY KEY,
    show_id  INTEGER NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    label    TEXT NOT NULL
);
