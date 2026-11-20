# Reliability plan

Goal: the show never depends on RF Ops being up, but RF Ops is up for the whole show.

**Tags:**
- **[have]** = already built. **[add]** = in the build plan.
- **V** = verified source. **I** = inference. Sources at the end.

## 1. Gaps found in the current build

1. **Stale data shows as live.** A half-open WebSocket (phone woken from sleep, AP roam) can show
   "● Live" over stale data. The server pings but never checks the pong, and the client has no
   heartbeat.
2. **Power loss can drop a swap.** SQLite runs WAL with `synchronous=NORMAL`, so "a transaction
   committed … might roll back following a power loss" (V). A swap could vanish. macOS fsync
   doesn't flush the drive cache without `fullfsync` (V).
3. **No database health checks.** There is no `busy_timeout`, no startup `quick_check`, and no
   checkpoint management.
4. **Backups are weak.**
   - VACUUM INTO writes straight to the final file name, so an interrupted backup looks valid.
   - There is no verification.
   - The show-start backup can be rotated away the same day (40 files kept).
   - Backups are never copied off the machine.
5. **No server after reboot.** The server is a LaunchAgent, so it only runs while someone is
   logged in. FileVault (on by default in recent macOS) disables auto-login, so after an
   unattended reboot there is no server (V).
6. **Clock skew.** `hello.server_time` is never used, so client clock skew distorts banners and
   ages.
7. **No graceful stop.** There is no SIGTERM handler, logs aren't rotated, and the idempotency
   table is never pruned.
8. **Snapshot load.** Every event makes every client fetch a full snapshot. Load grows as
   clients × events.
9. **Plain HTTP limits.** No service worker (offline shell), Wake Lock or notifications, and iOS
   suspends pages when the screen locks (V). See §4 for the HTTPS decision.

## 2. Failure modes and responses

| # | Failure | Detected by | Effect | Response | Left over |
|---|---|---|---|---|---|
| 1 | Server process crash | launchd; clients see the socket close | 2–15 s outage | [have] KeepAlive. [add] LaunchDaemon, ThrottleInterval 2, SIGTERM drain, crash counter | Crash loop on a bad DB → #5 |
| 2 | Mac sleeps | All clients stale | Full outage | [add] Mac mini; `pmset sleep 0`; server holds a sleep assertion; pre-show check reads `pmset -g assertions`; laptops: AC and lid open | Someone closes the lid |
| 3 | macOS update / restart | Uptime reset on the health page | Outage of minutes or more | [add] Updates off for the run; LaunchDaemon starts without login | Some security responses can't be deferred |
| 4 | Power loss | UPS alarm; unclean start in log | Outage; with NORMAL sync the last commits roll back | [add] UPS on Mac, switch and AP; `synchronous=FULL` + `fullfsync`; Mac mini restarts after power failure | UPS runs out |
| 5 | DB corruption | [add] `quick_check` at start and hourly | Won't serve | [add] Auto-restore the newest verified backup into a new file and keep the bad one; one-command restore | ≤ 2 min of edits lost in show mode (event log helps) |
| 6 | Disk full | [add] Free-space check every 60 s | Writes fail visibly (no silent loss) | [add] Alerts at 2 GB / 500 MB; log rotation; backup size cap | |
| 7 | Phone Wi-Fi drop / roam | [add] Heartbeat > 25 s → STALE | That phone is stale | [have] Reconnect + replay + idempotent retries. [add] Heartbeat staleness, pending queue (interface spec §11) | Bad RF floor |
| 8 | AP failure | All Wi-Fi clients stale | Phones dark; wired wall/FOH fine | [add] Wall and FOH wired; spare AP with the same SSID | AP swap time |
| 9 | Phone locks / backgrounds | Socket suspended | Stale until resumed | [add] On resume: drop socket, reconnect, re-fetch; reload if no hello within 5 s | Acting before the banner shows |
| 10 | Kiosk browser crash | Process exits | Wall blank | [add] LaunchAgent-kept Chrome kiosk; page watchdog reload; presence list shows the wall missing | GPU hang |
| 11 | iPad auto-lock / Guided Access | Presence lost | Wall dark | [add] Auto-Lock Never + Guided Access Display Auto-Lock Never | iPadOS update resets settings |
| 12 | Bridge crash / Playback Mac sleeps | [have] Heartbeat, 10 s alert | No MIDI follow | [have] Manual ◀ ▶. [add] Same sleep profile on the Playback Mac; log in ~/Library/Logs | Cue lost in that window |
| 13 | Playback Mac replaced | Bridge never online | No follow | [add] One install script + key; pre-show test cue; accept Local Network Privacy | |
| 14 | Network switch failure | Everything offline | Outage (Dante too) | [add] Spare labelled switch | Not RF Ops-specific |
| 15 | Clock skew | [add] Offset from hello/tick | Wrong ages | [add] All client time maths uses server time | — |
| 16 | Wrong show selected | Top bar | Edits to the wrong show | [add] Server-level **active show**; every screen follows it; switching asks for confirmation | |
| 17 | Accidental tap | Event log | Wrong ack / advance | [add] Undo; hold-to-advance on phones; big targets | |
| 18 | Conflicting edits | Event log | Silent overwrite | [add] Version check on mutations → "Sam just changed this" | |
| 19 | Load from many clients | Health page p99 | Slow updates | [add] Per-show deltas / coalesced refresh ≥ 250 ms; per-client rate limit; 30-client load test | |
| 20 | Half-open sockets | [add] Two-way heartbeat | "Live" on stale data | [add] Server tick every 10 s; client marks STALE at 25 s; server reaps at 30 s | — |
| 21 | Server Mac dies outright | Everything offline | Outage | [add] **Warm standby** (§3) plus paper fallback (interface spec §11.3) | Edits since the last backup copy |

## 3. Warm standby (second Mac)

This is the simplest pattern a crew can run (I). Treat it as a design to test, not a proven one.

**Normal running.** A second Mac has RF Ops installed but **stopped**. The primary copies each
verified backup to it within 60 s. In show mode, backups run every 2 min.

**Takeover.** `rfops-takeover` (one command, printed on the run sheet):
1. Confirm the primary doesn't answer `/api/health` three times over 6 s. If it answers, refuse.
2. Restore the newest verified backup and run `quick_check`.
3. Write a new **epoch** to the DB.
4. Take over the primary's IP (alias or DHCP reservation swap).
5. Start the server.

**After takeover.**
- Clients reconnect to the same address. A new epoch makes them do a full resync.
- **No split brain:** a primary that comes back and sees a higher epoch, or an IP conflict,
  starts **read-only**. The crew decides when to fail back.
- Target: < 2 min to takeover, drilled by a non-developer.

## 4. HTTPS decision (owner)

Plain `http://<ip>` works everywhere but can't use the following (V: MDN secure contexts):
- service workers, so there's no offline page after a reload;
- Wake Lock, so phones sleep;
- notifications;
- on iOS, Home-Screen apps can't notify.

`navigator.vibrate` doesn't exist on iOS at all.

**Option A, HTTP.**
- Phones are a **pull tool**: they show state when looked at.
- Alarms rely on the wall and FOH screens and on comms.

**Option B, local HTTPS.**
- Create a local certificate authority once and install it on each device (about 2 minutes per
  phone).
- This enables:
  - an offline shell;
  - Wake Lock;
  - installed-app notifications (iOS 16.4+);
  - automatic reconnect on resume.
- Costs: certificate setup for every new device, and expiry to manage (10-year local CA).

## 5. Requirements (testable)

### Database

1. **Durability.** `synchronous=FULL` + `PRAGMA fullfsync=ON` on every connection.
   *Test:* pull power 1 s after a swap returns, 20 times; the swap survives every time.
2. **Busy timeout.** `busy_timeout` ≥ 5000 ms on every connection.
   *Test:* a backup during writes causes no SQLITE_BUSY.
3. **Startup check.** `quick_check` at startup and hourly. On failure: move the bad DB aside and
   restore the newest verified backup, or refuse to start with a clear message.
   *Test:* a corrupted file.
4. **Atomic backups.** Write `*.tmp`, run `quick_check`, fsync, then rename. Failed backups never
   appear in the list.
   *Test:* kill during VACUUM INTO.
5. **Retention.** Show-start and manual backups are pinned. Automatic backups are capped at 7 days
   or a size limit.
   *Test:* 50 automatic backups don't remove the pinned one.
6. **Off-machine copy.** Copy each verified backup to the standby (or a USB drive) within 60 s.
7. **Checkpointing.** `wal_checkpoint(TRUNCATE)` after each backup; WAL size on the health page.
   *Test:* WAL < 10 MB after a 12 h soak.
8. **Disk space.** Checked every 60 s. Alerts at 2 GB and 500 MB; below 500 MB, backups pause and
   every screen alerts.

### Server process

9. **LaunchDaemon:** `/Library/LaunchDaemons`, `UserName=rfops`, `RunAtLoad`, `KeepAlive`,
   `ProcessType=Interactive`, `ThrottleInterval=2`.
   *Test:* reboot with nobody logged in; clients are live within 90 s.
10. **Graceful stop.** On SIGTERM: stop accepting, finish in-flight requests within 2 s, close the
    pool.
    *Test:* `launchctl kickstart -k` gives zero failed retried commands.
11. **Recovery time.** Restart to all clients resynced in < 20 s with 10 clients.

### Connections and clients

12. **Heartbeat.** The server sends `{type:"tick", seq, server_time, epoch}` every 10 s. A client
    with nothing for 25 s shows **STALE** and blocks risky writes (low-risk taps queue, interface
    spec §11).
13. **Reaping.** The server closes any socket silent for 30 s.
14. **Resume.** On visibilitychange, pageshow, online, or timer drift > 5 s: drop the socket,
    reconnect, re-fetch. No hello within 5 s → reload.
    *Test:* iPhone locked for 10 min is current within 10 s of unlocking.
15. **Staleness shown.** Seconds since the last server message are always visible; the header is
    red from 25 s.
16. **Server time.** All client time maths uses the server offset.
    *Test:* device clock 10 min fast; banners still expire correctly.
17. **Plain HTTP.** No secure-context-only features (unless Option B). Feature-detect optional
    APIs.
    *Test:* e2e against `http://<LAN IP>` in Safari and Chrome.
18. **Kiosk self-healing.** The kiosk page reloads if its main loop stalls for 60 s.
    *Test:* `kill` Chrome; the wall is back within 15 s.
19. **Presence.** The server keeps a list of connected screens (operator, role, last heartbeat),
    visible on FOH and Admin. In show mode, an expected screen missing for > 30 s raises a
    WARNING.
20. **Conflicting edits.** Mutations carry an expected version; stale writes get 409 with who
    changed it.
21. **Undo.** Undo is refused when a later event touched the same entity. A swap undo creates a
    FOH "swap undone" card.

### Load, soak, logs, standby

22. **Load.** 30 clients and 5 events/s for 10 min: p99 command < 300 ms, < 50% of one core.
23. **Soak.** 12 h with scripted events, 10 headless clients and a real iPad:
    - server RSS growth < 10%;
    - flat task count;
    - kiosk JS heap growth < 20%.
24. **Logs.** Rotate at 10 MB × 5 under `data/logs`. Prune idempotency rows older than 24 h.
25. **Standby.** Drilled takeover < 2 min.
26. **Health page.** `/health` shows:
    - uptime, epoch, DB quick_check result, WAL size, free disk;
    - last backup age and standby copy age;
    - connected screens, bridge state, sleep assertion, and pending OS updates where readable.

## 6. Show-day golden setup

### Server Mac

A Mac mini is preferred.

- **Network and power:** wired Ethernet, static IP, Wi-Fi off, on a UPS.
- **Commands:**
  - `sudo pmset -a sleep 0 displaysleep 10 disksleep 0 powernap 0 womp 1 autorestart 1 standby 0`
  - `sudo systemsetup -setrestartfreeze on`
- **macOS settings:**
  - Energy: "Prevent automatic sleeping" and "Start up automatically after a power failure".
  - Software Update: every automatic toggle off, including Security Responses.
  - Screen saver and password: Never.
  - Do Not Disturb on, AirDrop off, Screen Sharing on.
  - Time Machine and Spotlight excluded for `data/`.
- **Firewall:** off on the isolated show LAN, or sign the binary with a stable identity and allow
  it once. **No rebuilds on show day.**
- **Run as:** LaunchDaemon (`deploy/macos/com.rfops.server.daemon.plist`, to be added).
- **Pre-show health check:** run one command (`rfops-check`) or open the `/health` page.

### Playback Mac

- Same sleep and update profile as the server. Wired.
- The bridge stays a LaunchAgent (CoreMIDI needs the user session). That means:
  - auto-login on (FileVault off on this machine), or accept a manual login after a reboot;
  - accept the Local Network permission once;
  - log at `~/Library/Logs/rfops-bridge.log`.
- Pre-show: send the test cue.

### Wall screen

**Chrome on a mini PC or Mac:**
- Wired.
- A LaunchAgent with KeepAlive runs Chrome with:
  - `--kiosk --app=http://<server>/#/board?kiosk=1`
  - `--noerrdialogs --disable-session-crashed-bubble`
  - `--disable-background-timer-throttling --disable-backgrounding-occluded-windows --disable-renderer-backgrounding`
  - a dedicated `--user-data-dir`.
- Memory Saver off; auto-update off for the run.

**iPad as the wall:**
- Auto-Lock Never, on a charger, Low Power off, auto-updates off.
- Home-Screen web app.
- Guided Access with Display Auto-Lock Never, no time limit, and a passcode.
- Private Wi-Fi Address off for the show SSID.

### FOH screen

Laptop on wired Ethernet, Chrome `--app` window, Memory Saver off, no system sleep.

### Phones

- The RF Ops SSID only, with auto-join for others off.
- Auto-Lock ≥ 5 min or Never during the show.
- Home-Screen icon.
- Crew rule: **red STALE = don't trust it; wait or tap Reload.**

### Network

- **RF Ops Wi-Fi:** its own SSID/VLAN, or a dedicated AP port that excludes Dante. Confirm in
  Dante Controller that no media flows reach the AP.
- **Channels:** 5 GHz channels 36–64 only (GLX-D+ uses 5725–5850 MHz).
- **Spares:** a spare AP and a spare switch, pre-configured.
- **On site:** measure phone reconnect times and note them on the run sheet.

## Sources

- **SQLite:** pragma.html, wal.html, lang_vacuum.html, howtocorrupt.html, rescode.html
  (sqlite.org); Litestream tips.
- **launchd:** launchd.plist(5).
- **Apple:**
  - Support: preventing sleep, software update settings, FileVault and automatic login.
  - TN3179 (Local Network Privacy).
  - DTS forum threads 747298 / 776479 / 763753.
- **Show-machine prep:** Voxel "Make QLab Unstoppable"; Gareth Fry QLab setup;
  installation_up_4evr.
- **WebKit bugs** 228296 and 308073 (iOS WebSocket suspension and resume).
- **Browser:** MDN Secure Contexts; Chrome background-tab throttling and Memory Saver;
  chrome-launcher flags.
- **Dante:** Audinate on Wi-Fi and multicast; Yamaha Dante network design guide; Shure on Dante
  and IGMP snooping.
- **WebSocket heartbeats:** websocket.org; python-websockets keepalive docs.
- **Other:** Android DnsResolver (mDNS); chrony FAQ; GreptimeDB / Svix on Rust memory
  growth; tokio-metrics.
