-- Battery checks per device (R5: they count only for the show they were done before).
CREATE TABLE device_battery_checks (
    id          INTEGER PRIMARY KEY,
    device_id   INTEGER NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    result      TEXT NOT NULL CHECK (result IN ('ok', 'fresh', 'low')),
    checked_at  TEXT NOT NULL,
    checked_by  TEXT NOT NULL
);
CREATE INDEX device_battery_checks_dev ON device_battery_checks(device_id, checked_at);
