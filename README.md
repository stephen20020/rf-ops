# RF Ops

Backstage wireless mic & IEM operations system: equipment, performer assignments by song, handoffs,
checkout/return, battery checks, and alerts — shared live between backstage A2s and FOH on a local
network, with no internet required.

Status: **Phase 1 (core + data API)**. No UI yet. See [`docs/BRIEF.md`](docs/BRIEF.md) for the full
plan and [`docs/requirements.md`](docs/requirements.md) for the venue's known setup and open questions.

## Layout

| Path | What |
|---|---|
| `crates/core` | Pure domain logic — assignment overlay, conflicts, handoffs, battery rules, alert lifecycle. No I/O. |
| `crates/server` | Axum HTTP API + SQLite (sqlx). Every mutation writes an `event_log` row in the same transaction. |
| `docs/` | Brief, research, requirements. |

## Run

Requires Rust (stable).

```bash
cargo run -p rfops-server
```

Environment variables:

- `RFOPS_DB` — SQLite URL (default `sqlite://rfops.db`)
- `RFOPS_ADDR` — listen address (default `0.0.0.0:8080`)

Mutating requests need an `x-rfops-actor: <name>` header (PIN login replaces this in a later phase);
`x-rfops-client: <id>` is optional and is recorded in the event log.

## Test

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## Concepts

- **Kit** — equipment used together, per show: a fixed *base* (receiver channel / IEM transmitter
  channel / guitar receiver), a portable *carrier* (handheld, bodypack, IEM pack, guitar pack) and an
  optional *element* (capsule, headset, lav). Kits carry console channel, DCA and IEM mix.
- **Assignment** — performer → kit. Rows with no song are show defaults. Rows for a song override all
  defaults *of the same kind* (mic / iem / guitar) for that performer in that song; an override with
  no kit removes that kind for the song.
- **Handoffs** — computed from consecutive songs: `transfer`, `issue` (from stock), `return`, and
  `component_change` (capsule/headset swap on the same transmitter). Skipped songs are excluded.
- **Conflicts** — an item held by two performers in one song, unknown kits, kind mismatches, faulty or
  retired items in use. Assignments are saved even when they conflict; the response lists conflicts so
  the editor can show them.

## API (Phase 1)

| Method & path | Purpose |
|---|---|
| `GET /api/health` | Liveness |
| `GET/POST /api/equipment`, `GET/PATCH/DELETE /api/equipment/{id}` | Inventory (`?kind=&status=` filters) |
| `GET/POST /api/users` | Operators (roles: admin, a2, foh, viewer) |
| `GET/POST /api/performers`, `GET/PATCH/DELETE /api/performers/{id}` | Performers, A2 owner, zone |
| `GET/POST /api/shows`, `GET/PATCH/DELETE /api/shows/{id}` | Shows; `GET` returns songs, kits, assignments |
| `POST /api/shows/{id}/duplicate` | Copy a show as a template |
| `POST /api/shows/{id}/current` | Set current song (`source`: manual / midi) |
| `POST /api/shows/{id}/songs`, `POST /api/shows/{id}/songs/reorder`, `PATCH/DELETE /api/songs/{id}` | Setlist, skip, MIDI trigger |
| `POST /api/shows/{id}/kits`, `PATCH/DELETE /api/kits/{id}` | Kits (slot kinds validated) |
| `POST /api/shows/{id}/assignments`, `DELETE /api/assignments/{id}` | Assignments (returns conflicts) |
| `GET /api/shows/{id}/effective?song_id=` | Who has what in a song (defaults to current) |
| `GET /api/shows/{id}/conflicts` | All conflicts by song |
| `GET /api/shows/{id}/handoffs` | Full handoff plan |
| `GET /api/shows/{id}/upcoming?count=2` | Next transitions after the current song |
| `GET/POST /api/checkouts`, `POST /api/checkouts/{id}/return` | Checkout / return (`faulty` on return) |
| `GET/POST /api/battery-checks`, `GET /api/shows/{id}/battery-status` | Manual battery checks and due status |
| `GET/POST /api/alerts`, `POST /api/alerts/{id}/ack`, `POST /api/alerts/{id}/resolve` | Manual alerts and lifecycle |
| `GET /api/events?after=` | Event log (basis for real-time sync in Phase 2) |
