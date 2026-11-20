# RF Ops

Backstage wireless mic & IEM operations system: equipment, performer assignments by song, handoffs,
checkout/return, battery checks, and alerts — shared live between backstage A2s and FOH on a local
network, with no internet required.

Status: **Phase 2 (live sync + screens)**. Backstage wall/phone, FOH and Admin screens share state
in real time over WebSocket. See [`docs/BRIEF.md`](docs/BRIEF.md) for the full
plan and [`docs/requirements.md`](docs/requirements.md) for the venue's known setup and open questions.

## Layout

| Path | What |
|---|---|
| `crates/core` | Pure domain logic — assignment overlay, conflicts, handoffs, battery rules, alert lifecycle. No I/O. |
| `crates/server` | Axum HTTP API + SQLite (sqlx). Every mutation writes an `event_log` row in the same transaction; a broadcaster pushes new events to WebSocket clients. |
| `web` | Svelte 5 + TypeScript client (Backstage, FOH, Admin), served by the server from `web/dist`. |
| `docs/` | Brief, research, requirements. |

## Run

Requires Rust (stable) and Node 20.19+ (only to build the web client).

```bash
cd web && npm install && npm run build && cd ..
```

```bash
cargo run -p rfops-server -- --seed-demo
```

Open `http://<server-ip>:8080` on any device on the network. `--seed-demo` loads demo data
(ULXD4Q receivers, XSW and G4 IEMs, GLXD guitar packs, 11 performers, an 8-segment service) only
when the database is empty. Pick an operator on first load (e.g. *A2 Stage Left*).

Screens:

- **Backstage** (`#/backstage`) — wall touchscreen and A2 phones: current/next song, performer cards
  with kits and battery state, alerts with ack/resolve, upcoming handoffs to tick off, "Mine" filter
  for the A2's own performers, one-tap battery checks.
- **FOH** (`#/foh`) — channel list by console channel and IEM mix: who is on it now and next song,
  pack, battery, alerts.
- **Admin** (`#/admin`) — assignment grid (default + per-song overrides, conflicts in red), show,
  setlist and kits, inventory, performers and operators.

Environment variables:

- `RFOPS_DB` — SQLite URL (default `sqlite://rfops.db`)
- `RFOPS_ADDR` — listen address (default `0.0.0.0:8080`)
- `RFOPS_WEB` — built web client folder (default `web/dist`)

Mutating requests need an `x-rfops-actor: <operator name>` header. Until the first operator exists,
any name may act; after that the name must match an operator, and `viewer` operators are read-only.
`x-rfops-client` is recorded in the event log; `x-rfops-idempotency-key` makes retries safe.
(PIN sign-in comes in Phase 3.)

## Test

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

```bash
cd web && npm run check && npm run build && npm run test:e2e
```

The e2e suite drives three browsers against a real server: alert raise → FOH in < 1 s, ack and
resolve sync, song advance, handoff ticks, server kill/restart with offline blocking and resync,
48 px touch targets, and the viewer role. It needs Playwright with Chromium installed.

## Live sync

- `GET /api/ws?after=<seq>` — WebSocket. Sends `hello`, replays events after `seq` (or `resync` if
  too far behind), then streams `event` messages as they commit.
- `GET /api/snapshot?show_id=` — everything a screen needs in one call (clients refetch after events).
- Commands carry an idempotency key; a retried command returns the original response instead of
  applying twice.

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

## API

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
| `GET /api/events?after=` | Event log |
| `GET /api/snapshot?show_id=` | Full client state |
| `GET /api/ws?after=` | Live event stream (WebSocket) |
| `POST /api/shows/{id}/advance` | Next / previous non-skipped song |
| `POST /api/shows/{id}/handoffs/done` | Tick / untick a handoff |
| `PUT /api/shows/{id}/assignments/cell` | Set one assignment-grid cell atomically |
