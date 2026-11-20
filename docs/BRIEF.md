You are building "RF Ops": a local-network backstage wireless mic & IEM operations system. Read this whole brief before writing code. Work in phases; do not start a phase until the previous phase's acceptance tests pass.

## Context
- Hardware (confirmed):
  - Shure ULX-D **ULXD4Q** quad receivers (4 channels each), handheld transmitters w/ interchangeable capsules, bodypacks w/ headsets/lavs.
  - IEMs: **10× Sennheiser XSW IEM** systems (SR transmitter + EK bodypack) — no network interface → manual-only.
  - IEMs: **3× Sennheiser SR IEM G4 (300 series)** transmitters, confirmed G4 with Ethernet — covered by Sennheiser's published TI 1254 "Media Control Protocol for ew G4", UDP port 53212 (confirm the document explicitly includes the SR IEM G4). Transmitter-side status only (frequency, RF on/off, mute, input level where documented); EK bodypack battery is never available over the network (one-way RF) → manual.
  - Guitar wireless: **2× Shure GLXD6 (Z2 band, 2.4 GHz)** and **1× Shure GLXD6+ (Z3, dual-band)** pedal receivers with bodypacks — no network interface → manual-only (battery checks, readiness). These share spectrum with Wi-Fi; see network section.
  - Console: **Allen & Heath dLive DM64 MixRack + S7000 surface**. dLive's own Shure integration (firmware ≥ 1.70) may already connect to the ULX-D receivers to show battery/RF/audio on the surface.
  - Playback: MultiTracks Playback on a **MacBook** (macOS version TBD) with an **iConnectivity PlayAUDIO1U** interface. Primary MIDI route: Playback MIDI Out → macOS IAC Driver virtual bus → RF Ops MIDI bridge on the same Mac → server over the LAN. This does not depend on the interface; the PlayAUDIO1U is only a fallback route.
  - Network: **Dante audio and control share one network.**
- Scale: many performers, many handoffs per show, **multiple A2s** working simultaneously. Design for at least 60 performers, 12 receivers (48 channels), 20 IEM channels, 40 songs/segments, 200 handoffs per show, 10 concurrent clients.
- Users: several A2s (wall touchscreen + phones/tablets), and a FOH engineer (laptop/tablet). All see the same state in real time.
- No internet at runtime. Everything runs on one local server; clients are browsers.

## Stack (required)
- Rust (stable), Axum (HTTP + WebSocket), Tokio, SQLx with SQLite (WAL mode), serde.
- Frontend: Svelte + TypeScript (SvelteKit static adapter or Vite SPA), built to static files and embedded/served by the Axum binary. Installable PWA; no CDN assets; all fonts/icons bundled.
- MIDI: `midir` crate.
- Optional later: Tauri 2 shell wrapping the same frontend for Windows/macOS.
- Single binary + one SQLite file + one config file. Cargo workspace: `server`, `core` (domain logic, no I/O), `adapters/shure_ulxd`, `adapters/midi`, `web/`.

## MVP scope
1. Inventory: receivers (with channels), transmitters (handheld/bodypack), capsules, headsets/lavs, IEM transmitters (channels), IEM packs, earpieces optional. Each item: id, type, make/model, label/asset tag, serial, status (available, checked_out, in_use, faulty, retired), notes.
2. Performers: name, role, photo (uploaded file, stored locally), notes.
3. Shows → setlist of songs/segments (ordered, re-orderable). Each show has a current song pointer.
4. Assignments: per show, a default assignment per performer (mic kit + IEM kit), with per-song overrides. A "kit" = receiver channel + transmitter + capsule or headset/lav; IEM kit = IEM channel + pack.
5. Views:
   - Backstage (touch-first, large targets ≥ 48px, high contrast dark theme): current song, every performer's current kit, status, battery state, and "next handoffs" panel.
   - FOH: compact grid of channels ↔ performers, alerts list, current/next song, manual song advance.
   - Admin: inventory, performers, shows/setlists, assignments editor.
6. Handoffs: computed whenever a transmitter/pack/capsule changes hands between consecutive songs (or between show default and an override). Show who gives what to whom, before which song. Tech marks handoff done.
7. Checkout/return: check out a kit to a performer, return it, log time and tech.
8. Manual battery verification: per transmitter/pack, record "battery fresh / checked OK / replaced / low", battery type, timestamp, tech. Configurable "battery check due" rule (e.g. before every show, or N hours of use). Sennheiser IEM packs are manual-only — no telemetry.
9. Alerts: manual (tech raises: performer, item, severity, text) and automatic (from telemetry rules and from overdue battery checks). Lifecycle: open → acknowledged (by whom, from which client) → resolved (by whom, note). Alerts sync to all clients instantly; unacknowledged critical alerts are visually loud on both screens (no audio by default; optional chime setting).
10. Song advance: manual (next/prev/jump) from any client, plus MIDI (phase 4). Manual always wins.
11. Console mapping: every mic kit and IEM kit stores dLive input channel number + channel name, optional DCA, and (IEM) the monitor mix/aux it carries. FOH view is sortable by console channel.
12. Spares & emergency swap: designated spare kits per show. One-tap "Swap to spare" (from alert or performer card): pick spare → reassigns performer for the rest of the show (or this song only), marks failed item faulty, moves/annotates the alert, and shows FOH a prominent banner "Performer X now on console ch NN (was MM)". Must be doable in ≤ 3 taps.
13. A2 ownership: each performer (or zone/stage-side) is assigned to an A2. Each A2 can filter to "my performers", "my handoffs", "my alerts". Handoffs and alerts show the owning A2. Alerts for unowned items go to everyone.
14. Phone/tablet layout for roaming A2s (portrait, ≥ 360 px wide) in addition to the wall screen and FOH layouts. Optional browser notification/vibration for critical alerts on owned items.
15. Transmitter identity check: when telemetry reports a transmitter identity for a channel (ULX-D `TX_DEVICE_ID`, verify in docs), compare to the transmitter assigned in the current song; mismatch → warning alert ("wrong pack on ch NN?"). Capsule/element identity is never reported by hardware — manual only.
16. Live setlist changes: insert, skip, reorder songs during a show; jump to arbitrary song; "unscheduled segment" that keeps current assignments. A MIDI trigger that maps to no song in tonight's show is logged and shown as a warning, never silently ignored.
17. Batteries: track rechargeable batteries as items (ID, type, cycle count if telemetry provides it, health notes) and log AA consumption per show. Shure rechargeable fields (BATT_CHARGE/HEALTH/CYCLE) populate automatically where supported.
18. Reuse: show templates, duplicate a show, CSV import/export for inventory, performers, and assignments (spreadsheet round-trip).
19. Access: per-user PIN with roles (admin, A2, FOH, viewer). Viewers read-only. "Show mode" locks inventory/assignment editing except swaps, checkouts, battery checks, alerts.
20. Post-show report: alerts with timelines, swaps, battery replacements, faulty items, MIDI/manual song changes — export as CSV and printable HTML.

## Explicit exclusions
- No frequency coordination, no automatic frequency/gain/mute changes on any device. The system is read-only toward Shure hardware in the MVP. (A later SET capability, if ever, must be behind an explicit per-action confirmation and disabled during "show mode".)
- No general production management, scheduling, crew management, ticketing, general inventory, lighting/video, or console control.
- No cloud, accounts, or internet dependencies. Simple local PIN-based "tech name" identification is enough; no OAuth.

## Data model (minimum; refine in phase 1, document in docs/data-model.md)
- equipment(id, kind ENUM[receiver, receiver_channel, handheld_tx, bodypack_tx, capsule, headset, lav, iem_tx_channel, iem_pack], make, model, label, serial, parent_id NULL, status, notes)
  - receiver_channel.parent_id → receiver; telemetry is keyed by (receiver ip, channel number).
- kit(id, show_id, kind ENUM[mic, iem], receiver_channel_id/iem_tx_channel_id, transmitter_id/pack_id, capsule_id NULL, element_id NULL, console_channel INT NULL, console_channel_name NULL, dca NULL, iem_mix NULL, is_spare BOOL)
- performer(id, name, role, photo_path, notes, owner_a2_user_id NULL, zone NULL)
- user(id, name, pin_hash, role ENUM[admin, a2, foh, viewer])
- swap(id, show_id, performer_id, from_kit_id, to_kit_id, scope ENUM[song, rest_of_show], song_id, reason, alert_id NULL, at, by)
- show(id, name, date, current_song_id NULL, mode ENUM[setup, rehearsal, show])
- song(id, show_id, position, title, midi_trigger NULL)  -- midi_trigger e.g. {"type":"program_change","channel":16,"value":12}
- assignment(id, show_id, performer_id, kit_id, song_id NULL)  -- song_id NULL = show default; non-NULL = override for that song
- checkout(id, equipment_id, performer_id, out_at, out_by, in_at NULL, in_by NULL)
- battery_check(id, equipment_id, result ENUM[fresh, ok, replaced, low], battery_type, checked_at, checked_by, source ENUM[manual, telemetry])
- alert(id, show_id, severity ENUM[info, warning, critical], source ENUM[manual, telemetry, rule], equipment_id NULL, performer_id NULL, title, detail, dedupe_key, state ENUM[open, acknowledged, resolved], opened_at, acked_at, acked_by, resolved_at, resolved_by, resolution_note)
- event_log(seq INTEGER PK AUTOINCREMENT, at, actor, client_id, type, payload JSON)  -- append-only audit + sync source
- telemetry_latest(receiver_channel_id PK, updated_at, rf_level, antenna, audio_level, batt_bars, batt_runtime_min, tx_type, rf_int, encryption_warning, tx_mute, raw JSON)  -- in-memory primary, persisted periodically; no long-term history in MVP beyond alerts
Constraints: a transmitter/pack cannot be in two kits active in the same song; validation must report conflicts in the assignment editor rather than silently accept them.

## Assignment logic
- effective_assignment(show, song) = show defaults overlaid with that song's overrides, per performer.
- handoffs(show, song N→N+1) = items whose holder differs between effective_assignment(N) and (N+1), plus capsule/element swaps on the same transmitter. Output: item, from performer, to performer, before song N+1.
- "Upcoming handoffs" panel shows the next 2 song transitions; items flagged red if the receiving performer's kit has an open battery/telemetry alert.
- Unit-test this logic exhaustively in `core` (pure functions, no DB).

## Multi-client synchronisation
- Server is the single source of truth. Every mutation is a command over HTTP POST (or WS message) → validated in `core` → written in one SQLite transaction together with an event_log row → broadcast to all WebSocket clients as {seq, type, payload}.
- Clients hold a state snapshot + last seq. On (re)connect: client sends last seq; server replays events after it, or sends a full snapshot if the gap is too large.
- Commands carry a client-generated idempotency key so retries after reconnect don't double-apply (e.g. double ack).
- Conflict policy: last-write-wins for simple fields; state-machine guards for alerts (cannot ack a resolved alert; resolve implies ack).
- Show a visible connection indicator on every client; when disconnected, block mutations and show stale-data warning with age.
- Target: change visible on other clients < 250 ms on a LAN.

## Alert generation
- Rule engine in `core` over telemetry_latest + battery_check + schedule. Each rule produces a dedupe_key (e.g. "rf_int:rx3:ch2") so a condition opens at most one alert; it auto-resolves only if configured, otherwise stays until a tech resolves it.
- Initial rules (thresholds in config, not code):
  - RF_INT_DET = CRITICAL → critical.
  - ENCRYPTION_WARNING active → warning.
  - Transmitter lost (no RF / tx type none) for a channel assigned in the *current* song → critical; in a future song → warning.
  - RF level below threshold for > N seconds, or a downward trend over a sliding window → warning ("deteriorating RF"). Label this explicitly as a receiver-level indicator, not a spectrum measurement.
  - Battery bars ≤ threshold or runtime ≤ X minutes → warning/critical.
  - Battery check overdue before show mode → warning.
  - Receiver TCP connection lost → warning per receiver.
- Every alert shows affected equipment AND the performer(s) assigned to it in the current and next song.
- Hysteresis/debounce on every telemetry rule to avoid flapping.

## MIDI cue handling
- Topology: Playback runs on its own laptop, not the server. Build a tiny **MIDI bridge** binary (`bridge/`, Rust + midir, Windows/macOS) that runs on the Playback laptop, listens on a selected MIDI input (a virtual port such as macOS IAC / Windows loopMIDI that Playback sends to, or the PlayAUDIO interface's MIDI), and forwards messages to the server over WebSocket with timestamps, auto-reconnect, and a heartbeat. Server raises a warning alert if the bridge heartbeat is lost during show mode.
- Alternatives (document, support only if needed): DIN MIDI cable from the PlayAUDIO MIDI out into a USB-MIDI interface on the server; or RTP-MIDI from the PlayAUDIO interface (needs an RTP-MIDI driver on the server OS — avoid on Linux).
- `adapters/midi` on the server consumes the same normalized MIDI events from either the bridge or a local port. Hot-plug: retry port open every 2 s if missing.
- Mapping per song via song.midi_trigger, default Program Change on a configurable channel, value = song index; alternative: a single "next song" note/CC.
- On trigger: set current song, broadcast. Ignore duplicates within 500 ms.
- Manual override: a "MIDI follow" toggle on FOH and backstage. Any manual song change while MIDI follow is on shows who changed it; a "lock" toggle ignores MIDI entirely. Log every MIDI-driven change in event_log.
- Provide a built-in MIDI monitor screen and a virtual "simulate MIDI" button for testing without Playback.

## Shure telemetry adapter boundaries
- `adapters/shure_ulxd` implements a `TelemetrySource` trait: connect(receiver cfg) → stream of normalised `TelemetryUpdate` events; it never issues SET commands except `SET x METER_RATE`.
- BEFORE implementing: obtain the official ULX-D Command Strings document (https://www.shure.com/en-US/docs/commandstrings/ULXD, or the PDF from Shure's site) and save it to docs/vendor/. Implement only commands and value formats documented there. Write a docs/shure-ulxd-protocol.md that cites the doc section for every command used. Do not invent commands, value ranges, or sentinel values; where the doc is ambiguous, record it as an open question and verify against real hardware.
- Known from the official doc (verify again): TCP port 2202, ASCII frames `< GET x PROP >`, `< REP x PROP value >`, `< SAMPLE x ALL nn aaa eee >`; non-metered properties are pushed as REP on change; metering via `SET x METER_RATE ms` (100–99999, 0 = off); on connect send `< GET 0 ALL >` for a full snapshot.
- Parser: streaming, tolerant of partial frames and unknown properties (log and ignore). Fuzz/property-test the parser.
- Connection manager: one TCP connection per receiver, exponential backoff reconnect (1 s → 30 s cap), keepalive by periodic GET, mark stale after configurable timeout.
- A `SimulatedReceiver` (TCP server speaking the same protocol, scriptable: battery drain, RF fades, interference flags, disconnects) must exist before hardware testing and is used in CI.
- Record firmware (`FW_VER`) and model per receiver; surface it in admin.
- Receiver clients: the dLive console (its built-in Shure integration), Wireless Workbench, and RF Ops may all connect to the same ULXD4Q receivers. Use a conservative meter rate (default 1000 ms, configurable; 250 ms minimum) and never more than one connection per receiver from RF Ops. The bench test must cover all three clients connected at once.
- Do NOT integrate Wireless Workbench. Document in docs/operations.md that WWB remains the coordination/scanning tool and that a hardware bench test must confirm WWB and RF Ops can both connect to the same receivers simultaneously without errors or dropped updates.
- Future (not MVP): import WWB-exported scan/coordination files to display backup-frequency suggestions read-only; any frequency change remains a human action in WWB/at the hardware.

## Offline deployment
- One server host (mini PC or Mac) on the production LAN with a static IP; serves `http://<ip>:<port>`. Optional mDNS name.
- Network (Dante + control shared): RF Ops traffic is small unicast TCP; it must never send multicast/broadcast beyond optional mDNS. Document recommendations in docs/network.md: static IPs (or DHCP reservations) for receivers and server; Wi-Fi access points for A2 phones should be on a separate SSID/VLAN routed to the server, or at minimum have multicast-to-unicast / IGMP snooping so Dante multicast never floods Wi-Fi; enable QoS/DSCP on switches per Dante guidance.
- Server host and the network switch for receivers on a UPS. Document power-loss recovery.
- Wi-Fi vs. GLX-D: GLX-D (Z2) runs in 2.4 GHz and GLX-D+ also uses a second band (confirm exact range from the GLXD6+ user guide; reportedly 5.8 GHz). Docs must recommend: A2 Wi-Fi APs 5 GHz only, on channels that don't overlap the GLX-D+ second band, kept physically away from the guitar pedal receivers; no 2.4 GHz SSID during shows. Add a "Wi-Fi plan" checklist item to show-mode entry.
- Clients: wall touchscreen in kiosk-mode browser (Chromium `--kiosk`), FOH browser or Tauri app. PWA caches the shell so a reload during a brief outage still shows the last state flagged stale.
- No external network calls at runtime (add a test that fails if any outbound request is attempted).
- Config file (TOML): port, receivers (name, IP, channel count), MIDI port and mapping, thresholds, battery-check rules.
- Install docs for Windows, macOS, Linux; run as a service (systemd / launchd / Windows service via `windows-service` crate or NSSM).

## Failure handling & recovery
- SQLite WAL, synchronous=NORMAL, automatic backup copy at show-mode entry and every 15 min to a rotating folder; one-click export/import of a show (JSON).
- Server crash/restart: state restored from DB; clients auto-reconnect and resync; current song persisted.
- Receiver offline: adapter keeps retrying; UI shows "no telemetry" distinctly from "OK" (never imply health without data).
- MIDI port lost: alert + auto-retry; manual advance always available.
- Clock: all timestamps server-side.
- Graceful degradation: the manual workflow must work with zero receivers and zero MIDI connected.

## Phases & acceptance tests
Phase 1 — Core + data (no UI beyond a minimal admin): schema, migrations, `core` domain logic, CRUD API. Tests: assignment overlay, conflict detection, handoff computation (≥ 20 cases incl. capsule swaps and performers absent from songs), checkout/return, battery-check due rules.
Phase 2 — Real-time sync + UIs: WebSocket event bus, backstage/FOH/admin views, manual alerts lifecycle, manual song advance. Tests: Playwright with two browser contexts — change in one appears in the other < 1 s; ack from FOH reflects backstage; kill server mid-session and restart → clients resync with no lost or duplicated events; touch targets ≥ 48 px at 1080p.
Phase 3 — Show-ready manual MVP hardening: backups, export/import, kiosk mode, stale indicators, battery-overdue rule alerts, install docs. Acceptance: run a full mock show (10 performers, 15 songs, ≥ 8 handoffs) entirely manually on two clients without errors.
Phase 4 — MIDI: midir adapter, mapping UI, MIDI monitor, follow/lock toggles. Tests: simulated MIDI port drives song changes; duplicate suppression; lock ignores MIDI; port unplug/replug recovery. Manual test with MultiTracks Playback MIDI Out (requires Playback MIDI Out to be enabled on the user's account).
Phase 5 — Shure ULX-D telemetry: protocol doc review, parser + simulator, connection manager, telemetry rules, UI meters. Tests: parser fuzzing; simulator scenarios produce exactly one alert per condition with correct performer mapping; reconnect after receiver reboot; 16 channels at 100 ms meter rate with < 5% CPU on target host. Hardware bench test checklist (docs/bench-test.md): firmware recorded; simultaneous WWB + RF Ops connection; battery fields per transmitter type; RF_INT_DET behaviour; disconnect/reconnect.
Phase 5b — Sennheiser SR IEM G4 transmitter status (3 units, confirmed Ethernet): obtain Sennheiser TI 1254 "Media Control Protocol ew G4" PDF into docs/vendor/, implement only documented commands in `adapters/sennheiser_g4` behind the same `TelemetrySource` trait (UDP 53212, respect the documented 1500-character message limit), read-only. Rules: IEM transmitter RF off / muted / unreachable while assigned in the current song → critical. Bench-test coexistence with Sennheiser WSM.
Phase 6 (optional) — Tauri 2 packaging for FOH; read-only import of WWB coordination exports.
Phase 7 (optional) — dLive link, read-only: obtain Allen & Heath "dLive MIDI Over TCP Protocol" (current version, from allen-heath.com) into docs/vendor/; implement only documented messages. Use it to read input channel names and compare with RF Ops' console mapping (mismatch → info alert), optionally show channel mute state on FOH. Never write to the console. Respect any documented/observed MixRack connection limits and rate limits; test against dLive Director offline mode first.

Additional tests for scale and roles: seed 60 performers / 48 channels / 40 songs / 200 handoffs and verify the handoff panel and assignment editor stay responsive (< 100 ms render); emergency swap completes in ≤ 3 taps and FOH banner appears on all clients < 1 s; A2 filters show only owned items; viewer role cannot mutate; show mode blocks editing but allows swaps/checkouts/battery checks/alerts.

## Before Phase 1: discovery (do with the user)
- Produce docs/requirements.md by walking through one real show with the user: how A2s split work, current paperwork (mic plot/handoff sheets), spare procedure, battery routine, what FOH needs on screen. Import their existing spreadsheets as seed data. Confirm open items: SR IEM G4 IP addressing on the shared network, GLXD6+ second-band frequency range, PlayAUDIO model, Playback laptop OS, receiver count/firmware, whether dLive Shure integration is in use, touchscreen hardware.
- Produce low-fidelity wireframes for wall screen, phone, and FOH; get user approval before building UI.

## Working rules
- Keep domain logic pure and tested in `core`; adapters behind traits.
- `cargo clippy -- -D warnings`, `cargo test`, and frontend `npm run check && npm test` must pass at the end of every phase.
- Commit at the end of each phase with a summary of what passed.
- If something in this brief conflicts with official Shure documentation, follow the documentation and note the change.
