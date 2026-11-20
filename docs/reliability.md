# Reliability plan (v3, cloud)

The goal is that the show never depends on RF Ops being up, while RF Ops stays up for the whole
show.

**Where things run:**
- RF Ops runs on **Render**: one paid instance with a persistent disk. DNS and the R2 backup
  bucket are on **Cloudflare** (`docs/hosting-and-security.md`).
- On site, the only RF Ops software is the **agent** (`rfops-bridge`) on the Playback MacBook. It
  handles MIDI now; Phase 5 adds Shure telemetry.
- Every screen is a browser.

**Tags:**
- **[have]** = built and tested; **[add]** = in the build plan.
- **V** = verified source; **I** = inference.

## 1. Failure modes and responses

| # | Failure | Detected by | Effect | Response | Left over |
|---|---|---|---|---|---|
| 1 | Server process crash | Render restarts it; clients see the socket close | 10–60 s outage | [have] SIGTERM handling, WAL checkpoint on exit, startup quick_check. [add] Crash counter on /health | Crash loop on a bad DB → #5 |
| 2 | Render deploy or instance replacement | Socket close, new epoch unchanged | ~30–90 s outage (persistent disk disables zero-downtime deploys, V) | [add] **Deploy freeze:** auto-deploy off; deploys never during a show day. Runbook date check | Render platform maintenance |
| 3 | Venue internet loss | Every venue-network screen goes OFFLINE; phones on cellular still work | Wall/FOH dark | [add] §3.4 "Venue screens offline" mode, then paper fallback (interface spec §11.3). Survey cellular per venue | Phones with no cellular signal |
| 4 | Render region / platform outage | External uptime check; every client offline | Full outage | Paper fallback. Standby only covers a broken instance, not a region | — |
| 5 | DB corruption | [have] quick_check at start and hourly | Won't serve | [have] The bad file is moved aside and the newest verified local backup restored. [add] If none is local, restore the newest from R2 | ≤ 2 min of edits; client replay (§3.3) recovers acked commands |
| 6 | Disk full (5 GB disk) | [add] Free-space check every 60 s | Writes fail visibly, never silently | [add] Retention §2.2; WARNING at 25 % free, CRITICAL at 10 % (local backups pause, R2 uploads continue) | |
| 7 | Phone Wi-Fi drop or roam | [add] No message for 25 s → STALE | That phone is stale | [have] Reconnect, replay, idempotent retries. [add] STALE/OFFLINE states, IndexedDB queue (interface spec §11) | Bad RF floor |
| 8 | Venue AP failure | Wi-Fi clients stale | Phones fall back to cellular if allowed | [add] Wall/FOH wired; spare AP | |
| 9 | Phone locks or backgrounds | Socket suspended | Stale until resumed | [add] On resume: drop the socket, reconnect, re-fetch. Reload if no hello within 5 s | Acting before the banner shows |
| 10 | Kiosk browser crash | Presence | Wall blank | [add] Page watchdog reload; presence lists the wall as missing | GPU hang |
| 11 | iPad auto-lock / Guided Access | Presence lost | Wall dark | Auto-Lock Never and Guided Access (§5) | iPadOS update resets settings |
| 12 | Agent crash, or the Playback Mac sleeps | [have] Bridge heartbeat, 10 s alert | No MIDI follow | [have] Manual ◀ ▶. [add] Last-cue buffer (§3.5); sleep profile on the Playback Mac | |
| 13 | Clock skew | [add] Offset from hello and tick | Wrong ages | [add] All client time maths uses server time | — |
| 14 | Wrong show selected | Top bar | Edits to the wrong show | [add] Server-level **active show**; switching asks for confirmation | |
| 15 | Accidental tap | Event log | Wrong ack or advance | [add] Undo; hold-to-advance on phones; big targets | |
| 16 | Conflicting edits | Version check | Silent overwrite | [add] 409 "Sam just changed this" | |
| 17 | Many clients | /health p99 | Slow updates | [add] Coalesced refresh ≥ 250 ms; per-client rate limit; load test | |
| 18 | Half-open sockets | Two-way heartbeat | "Live" on stale data | [have] Server tick every 10 s; reap after 30 s. [add] Client STALE at 25 s | — |
| 19 | Instance broken beyond restart (bad disk, bad deploy) | Uptime check; Lead | Outage | [add] **Standby takeover** (§3), < 10 min | Edits after the last R2 copy, less what client replay recovers |
| 20 | Account or credential compromise | Audit log, rate-limit alert | Bad data | `docs/hosting-and-security.md` §Security | |

## 2. Database and backups

### 2.1 Database [have]

- `journal_mode=WAL`, `synchronous=FULL`, `fullfsync=ON`, `busy_timeout=5000` on every connection.
- `quick_check` at startup and hourly. A corrupt file is moved aside as `*.corrupt-<stamp>.db`.
  Then:
  - the newest verified backup is restored;
  - or, if none exists, the server refuses to start with "no verified backup".

### 2.2 Backups and retention (5 GB persistent disk)

**Cadence.** Every 2 min in show mode, 15 min otherwise, skipped when nothing has changed.
Always taken at show start, and by hand.

**Atomic write.** `VACUUM INTO` a temp file → `quick_check` → fsync → rename. Temp files are
never listed. [have]

**Retention:**

| Copy | Automatic backups | Pinned backups (show start, manual, pre-restore, pre-migration) |
|---|---|---|
| Local | Newest **20** | 30 days |
| R2 (bucket lifecycle rules) | 14 days | 90 days |

**Off-site.** Each verified backup is uploaded to R2 straight after writing. The last upload
time and error are shown on /health. Two failed uploads in a row → WARNING; 10 min without a
successful upload in show mode → CRITICAL.

**Free space.** Checked every 60 s on the disk holding the DB.
- **< 25 % free:** WARNING.
- **< 10 % free:** CRITICAL. Local backups pause, but R2 uploads of a fresh backup continue
  (written, uploaded, then deleted locally).
- Logs go to stdout (Render keeps them); nothing grows on disk except the DB and backups.

**Restore.** Admin → Data → Restore (Lead + PIN). It takes a pinned "pre-restore" backup first.
Sources: a local backup, R2, or an uploaded file. Then a new **epoch** is written, so every
client resyncs (§3.3).

## 3. Connections, epoch, standby

### 3.1 Heartbeat and staleness

- The server sends `{type:"tick", seq, server_time, epoch}` every 10 s. [have]
- The client sends a ping every 10 s; the server reaps a socket silent for 30 s. [have, server]

**Client states:**

| State | When | What the client does |
|---|---|---|
| **LIVE** | Messages arriving | Normal |
| **STALE** | Nothing for 25 s | Header turns red; risky writes are blocked, low-risk taps queue |
| **OFFLINE** | Nothing for 60 s | Offline banner |
| **Paper** | 120 s offline | "Use the paper plot" instruction |

The client tells apart **device offline** (`navigator.onLine` false, or a health fetch fails at
the network level) from **server unreachable** (network up, server not answering). Each has its
own wording.

### 3.2 Resume and server time

- On visibilitychange, pageshow, online, or timer drift > 5 s: drop the socket, reconnect,
  re-fetch. No hello within 5 s → reload.
- All client time maths (ages, banners, "checked 13:02") uses `server_time` offset.

### 3.3 Epoch and client replay

The **epoch** (settings `server.epoch`) increments on restore and on standby takeover.

**When a client sees a new epoch** in hello:
1. It discards its snapshot.
2. It re-sends every command that was **acknowledged in the last 15 min** (kept in IndexedDB with
   its idempotency key and the version it expected).
3. The server applies each one if its idempotency key is unknown. A key it already knows is a
   no-op.
4. A stale version gets a 409, and the client lists the conflicts once: "Couldn't re-apply: Red
   battery OK (Sam changed it)".

This recovers the changes between the last backup and the failure, as long as some client saw
them acknowledged.

**Never replayed:**
- show-mode switches;
- song advances (the current song is re-established by MIDI or the Lead);
- admin edits.

### 3.4 Venue screens offline (venue internet down, phones on cellular)

The server knows which screens are expected (wall and FOH, enrolled screen credentials) from
presence.

**If every venue-network screen is gone for > 60 s while phones are still connected:**
- Phones show a top banner: **"Venue screens offline — FOH can't see swaps."**
- One combined **NOTE** replaces the per-screen presence warnings.
- Swaps go straight to **"FOH confirm pending."** The swap card stays at the A2 with
  "Tell FOH on comms: <copy-paste line>". When FOH is back, it gets the pending cards and
  confirms them.
- MIDI follow stops (the agent is on the venue network). The Lead advances by hand with
  hold-to-advance.

**When the venue screens are back,** the banner clears and FOH sees the pending swap cards.

### 3.5 Agent last-cue buffer

- The agent keeps the last cue it received, with the local receive time, while the server is
  unreachable.
- On reconnect it posts that cue with `cued_ms_ago`.
- **Under 10 s old:** applied normally.
- **10 s or older:** the server raises a **Song check** prompt to the Lead: "Playback moved to
  *Song 6* while offline. Go there?" [Go] [Stay].
- Older buffered cues are dropped; only the last one counts.

### 3.6 Standby takeover (D7, cloud)

**Setup:**
- Two Render services from one Blueprint: `rfops` (active) and `rfops-standby` (suspended), each
  with its own disk.
- **Traffic goes through a Cloudflare Worker** on `rfops.<domain>/*`. It forwards every request,
  WebSockets included, to the host named in its `ORIGIN` variable (`rfops.onrender.com` normally).
  - Flipping `ORIGIN` takes effect within seconds.
  - Render needs no custom-domain move or new certificate. Render routes by Host header, so
    moving a custom domain between services means removing it, re-adding it and waiting for a
    certificate. That is too slow and uncertain for a show.
  - The Worker adds nothing else: no caching, no rewriting.
- Both services share:
  - `RFOPS_SESSION_KEY`, so cookies stay valid;
  - `RFOPS_PUBLIC_ORIGIN`;
  - the R2 credentials.

**Lease** (R2 object `lease.json` = `{generation, holder, renewed_at}`):
- R2 supports conditional `PutObject` with `If-Match` / `If-None-Match`, evaluated atomically,
  and returns 412 to the loser (V: developers.cloudflare.com/r2/api/s3/api/).
- `generation` is the lease's own counter, separate from the DB epoch.
- `holder` = `RFOPS_INSTANCE` (`active` | `standby`).

| Rule | Behaviour |
|---|---|
| Any service at start | If the lease already names it (including the standby after its own Take over), it resumes the lease |
| Very first start | No lease object at all: `active` creates generation 1 |
| Otherwise | **Starts fenced**, even if the lease has expired. Only **Take over** gives a service a lease it doesn't hold, and Take over always restores the newest backup first. So no service can come up writing on a stale disk |
| R2 unreachable at start | Serves only if its database records that it held the lease last time (an ordinary restart during an R2 outage), with a WARNING. Otherwise fenced |
| Take over | Writes generation + 1 (`If-Match` the read etag). It stays fenced until it has restarted into the restored database |
| Renewal | Every 15 s, `If-Match` the etag it last wrote |
| **Fence test** | A renewal gets 412 and the lease now names another holder → fence at once |
| Fenced | **Serves no data**: every `/api/` call except `/api/health` and `/api/standby/*` gets 503, and WebSockets are refused or closed. Screens show "This is not the active RF Ops server". Its disk may be empty, from a drill, or stale. R2 uploads stop |
| R2 unreachable | Keep serving and keep retrying. After 60 s without a successful renewal, a WARNING on /health. The runbook's first step (suspend the old service) is the real guard |
| Backup keys | `auto/g000004/<file>` and `pinned/g000004/<file>`: the class first so lifecycle rules match, then the generation zero-padded. Restore picks the newest backup of the **highest generation** (compared as numbers), so a fenced or stale instance's backups can never be chosen |

**Runbook** (a Lead, not a developer; target < 10 min, drilled once before the event):
1. Render → `rfops` → **Suspend**, if Render shows it running. This guarantees the old
   instance is gone.
2. Render → `rfops-standby` → **Resume**. It starts fenced.
3. Open `https://rfops-standby.onrender.com/#/takeover`. This page is allowed from the
   service's own address. It shows the backup it will restore and its age, and warns if that is
   older than 15 min (beyond the screens' replay window). Enter the Lead's name and PIN, then
   tap **Take over**. The standby:
   - downloads the newest verified backup of the highest generation from R2;
   - runs `quick_check`;
   - **checks the name and PIN against that backup's users** (its own disk may be empty);
   - writes the lease (`If-Match` the etag it read; generation + 1);
   - restarts into the restored database with a new epoch, and resumes its lease.

   **Take over needs R2.** If R2 itself is down, there is nothing to restore from: use paper.
4. Cloudflare → Workers → `rfops-proxy` → set `ORIGIN` to `rfops-standby.onrender.com`.
5. Clients reconnect, see the new epoch, and replay their acked commands (§3.3).
6. **Fail back** only after the show: resume `rfops` (it starts fenced), run **Take over** on
   `rfops.onrender.com/#/takeover`, set `ORIGIN` back, then suspend the standby.

Clients replay when **(generation, epoch)** changes, not the epoch alone: a restore followed by a
takeover can repeat an epoch number.

**Sign-in data across a takeover.**
- Any revocation, PIN change or join-code change triggers an immediate backup and R2 upload
  [have: backup; R2 upload in step 1b].
- The newest backup therefore already contains it, and a revoked session can't come back.

## 4. Requirements (testable)

### Database and backups

1. **Durability** [have]. `synchronous=FULL` and `fullfsync` on every connection.
   *Test:* pragma test; kill -9 one second after a swap returns, 20 times, and the swap survives
   every time.
2. **Busy timeout** [have]. ≥ 5000 ms. *Test:* a backup during writes causes no SQLITE_BUSY.
3. **Startup check** [have] plus R2 fallback [add]. *Test:* a corrupted file, with and without
   a local backup.
4. **Atomic backups** [have]. *Test:* an interrupted temp file is never listed.
5. **Retention** [add]. Local: 20 automatic, pinned kept 30 days.
   *Test:* 30 automatic backups leave 20, and the pinned one survives.
6. **Off-site** [add]. Each verified backup is uploaded to R2 within 60 s; failures show on
   /health and alert per §2.2. *Test:* against an S3-compatible mock.
7. **Checkpointing** [have]. WAL < 10 MB after a 12 h soak.
8. **Disk space** [add]. Thresholds at 25 % and 10 %. *Test:* injected free-space values.

### Server process

9. **Render health check** [add]. `/api/health` returns 200 whenever the process can answer,
   even if the DB is degraded, so Render never restart-loops a server that is serving. Detail is
   on /api/health/detail.
10. **Graceful stop** [have]. On SIGTERM: in-flight requests finish, then the WAL is checkpointed
    and the pool closed. *Test:* SIGTERM during writes gives zero failed retried commands.
11. **Recovery time.** Restart to all clients resynced in < 20 s with 10 clients.
12. **Deploy freeze** [add]. Auto-deploy is off in render.yaml. /health shows the build
    version, and the pre-show checklist confirms it matches the rehearsed version.

### Connections and clients

13. **Heartbeat** [have, server]. Client STALE at 25 s, OFFLINE at 60 s, paper at 120 s [add].
    *Test:* drop ticks; the states appear at the right times.
14. **Reaping** [have]. 30 s.
15. **Resume** [add]. *Test:* a page hidden for 10 min is current within 10 s of showing
    (Playwright visibility emulation), plus a manual iPhone check.
16. **Server time** [add]. *Test:* client clock 10 min fast; banners still expire correctly.
17. **Replay never alarms** [add]. On load, no toast, vibration or pulse for events with seq ≤
    the hello seq, except still-NEW criticals owned by this operator (once).
18. **Epoch replay** [add]. *Test:*
    - restore a backup missing the last 3 commands;
    - the client replays them, 2 apply and 1 gives a 409;
    - the conflict is listed once.
19. **Kiosk self-healing** [add]. The kiosk page reloads if its main loop stalls for 60 s.
20. **Presence** [have, server]. In show mode, an expected screen missing for > 30 s raises a
    **NOTE** [add]. Every venue screen missing combines into the §3.4 banner.
21. **Conflicting edits** [add]. Expected version on mutations; 409 with who changed it.
22. **Undo** [add]. Refused when a later event touched the same entity. A swap undo creates a FOH
    "swap undone" card.
23. **Agent cue buffer** [add]. *Test:* a cue posted with `cued_ms_ago` 3000 applies; one with
    30000 raises the Song check.

### Load, soak, standby

24. **Load.** 30 clients and 5 events/s for 10 min: p95 command < 300 ms and p99 < 1 s on the
    Render instance type chosen.
25. **Soak.** 12 h with scripted events and 10 headless clients:
    - server RSS growth < 10 %;
    - kiosk JS heap growth < 20 %.
26. **Standby.**
    - Fencing tests against an S3 mock:
      - the standby always starts fenced;
      - the active starts fenced when the lease is fresh and held by the standby;
      - a renewal that gets 412 with another holder fences and refuses writes;
      - two concurrent takeovers end with exactly one winner;
      - restore picks the highest generation.
    - Takeover refuses a wrong PIN (checked against the downloaded backup).
    - A drilled takeover in < 10 min (manual, once before the event).
27. **Health detail (Lead only)** shows:
    - uptime, epoch, version, DB quick_check, WAL size, free disk %;
    - last backup and last R2 upload ages;
    - lease holder;
    - connected screens and agent state.

## 5. Show-day golden setup

### Cloud

- Confirm the Render instance is paid tier (the free tier spins down and drops sockets, V).
- **Day before:** check the version matches the rehearsal, auto-deploy is off, and the last R2
  upload is < 5 min old.
- An external uptime check (Cloudflare health check or UptimeRobot) on `/api/health` pages the
  Lead.

### Playback MacBook (agent)

- Wired. On AC, lid open.
- `sudo pmset -a sleep 0 disksleep 0 powernap 0 standby 0` for the run.
- Software updates off for the run.
- The agent is a LaunchAgent: CoreMIDI needs the user session. Accept the Local Network
  permission once.
- Pre-show: send the test cue.

### Wall screen

**Chrome on a mini PC or Mac:**
- Wired.
- `--kiosk --app=https://rfops.<domain>/#/board?kiosk=1`, plus:
  - `--noerrdialogs --disable-session-crashed-bubble`
  - `--disable-background-timer-throttling --disable-backgrounding-occluded-windows --disable-renderer-backgrounding`
  - a dedicated `--user-data-dir`.
- Memory Saver off.
- Enrolled with a **screen credential** (interface spec §1.1).

**iPad as the wall:**
- Auto-Lock Never, on a charger, Low Power off.
- Home-Screen web app.
- Guided Access with Display Auto-Lock Never.

### FOH screen

Laptop, wired, Chrome `--app` window, Memory Saver off, screen credential.

### Phones

- Home-Screen app, signed in by join code or PIN.
- Auto-Lock ≥ 5 min.
- Crew rule: **red STALE = don't trust it; wait or tap Reload.**
- **Per venue:** survey whether cellular works backstage (§3.4) and note it on the run sheet.

### Network

- 5 GHz Wi-Fi on channels 36–64 only (GLXD6+ uses 5725–5850 MHz).
- Keep phone Wi-Fi off the Dante network.
- Spare AP.

## Sources

- **SQLite:** pragma.html, wal.html, lang_vacuum.html, howtocorrupt.html.
- **Render docs:**
  - persistent disks (no zero-downtime deploy);
  - free-tier spin-down;
  - WebSocket behaviour on deploy;
  - `maxShutdownDelaySeconds`;
  - health checks.
- **Cloudflare docs:** proxied DNS records; R2 conditional requests (`If-Match`) and lifecycle
  rules.
- **WebKit bugs** 228296 and 308073 (iOS WebSocket suspension and resume).
- **MDN:** Page Visibility, Secure Contexts, Push API.
- **Dante and Wi-Fi:** Audinate on Wi-Fi and multicast; Shure on IGMP snooping.
