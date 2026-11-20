# Hosting and security (cloud, owner decision Nov 2026)

## Decision

RF Ops runs **in the cloud only**, on **Render**, at a subdomain of the owner's domain (DNS on
**Cloudflare**). The venue has **reliable wired internet**. The owner accepts that RF Ops is
unavailable if venue internet fails. The fallback is the printed sheets and comms (interface spec
§11). The original brief's "operate without internet" no longer applies.

Why Render and not Vercel or Cloudflare as the host:
- RF Ops is one long-running Rust server holding WebSockets and a SQLite file.
- Render web services run exactly that.
- **Vercel's serverless functions can't hold WebSockets or a local database.**
- **Cloudflare Workers would need a rewrite** around Durable Objects.

Cloudflare is still used for DNS and for **R2 off-site backups**.

## Render facts that shape the design

| Fact | Source | Consequence |
|---|---|---|
| Free web services spin down when idle; free-tier WebSockets drop around 5 min (known bug) | Render community staff; python-websockets deploy guide | **Paid instance only** (Starter or above). Never free tier |
| WebSockets have no fixed timeout but close when the instance is replaced (deploy/restart) | render.com/docs/websocket | Clients must reconnect and resync (built; reliability reqs 12–14) |
| Attaching a persistent disk **disables zero-downtime deploys**; the old instance stops before the new one starts; one instance only | render.com/docs/disks | Every deploy = a short outage (tens of seconds). **Deploy freeze on show days**; deploy only with no show in show mode |
| Shutdown: SIGTERM, then SIGKILL after 30 s (configurable up to 300 s via `maxShutdownDelaySeconds`) | render.com/docs/deploys | Graceful SIGTERM handler (reliability req 10); set the delay to 30 s |
| Render issues TLS certificates for custom domains | Render custom-domains docs | Real HTTPS with no local certificates. **This resolves decision D5**: phone notifications, keep-awake and an offline shell all work |

## Architecture

```
 Venue LAN (wired internet)                      Cloud
 ┌─────────────────────────────┐                ┌──────────────────────────────┐
 │ Wall screen  ─┐             │   HTTPS/WSS    │ Render web service (1 inst.) │
 │ FOH laptop   ─┼─────────────┼───────────────▶│  rfops-server + SQLite on    │
 │ A2 phones    ─┘ (Wi-Fi or   │                │  persistent disk             │
 │   cellular)                 │                │                              │
 │ Playback MacBook:           │   HTTPS (out)  │                              │
 │   rfops-agent (MIDI; later  ├───────────────▶│                              │
 │   ULX-D + G4 telemetry)     │                └──────────┬───────────────────┘
 │ ULX-D receivers, G4 TXs ◀───┤ LAN only                  │ verified backups
 └─────────────────────────────┘                ┌──────────▼──────────┐
                                                │ Cloudflare R2 bucket │
                                                └─────────────────────┘
```

**Clients:**
- The wall and FOH screens use wired internet.
- **Phones can fall back to cellular** if the venue Wi-Fi fails. Cellular is an upside of cloud
  hosting.

**The on-site agent (today's MIDI bridge, extended in Phase 5):**
- It makes **outbound** connections only. No ports open at the venue.
- It reads Playback MIDI now.
- In Phase 5 it also polls the ULX-D receivers (TCP 2202) and the G4 transmitters (UDP 53212) on
  the LAN, and streams the readings up.
- **The on-site agent is needed for any receiver telemetry, because a cloud server can't reach
  devices on the venue LAN.**

**Latency.** Internet round-trips (typically 20–80 ms) sit well inside the < 1 s screen-update
target. This is confirmed by the e2e timing tests run against the deployed URL.

## Backups and the standby

Full detail is in `docs/reliability.md` §2.2 and §3.6.

**Backups:**
- Verified backups every **2 min in show mode** and every 15 min otherwise. Each is uploaded to a
  private **Cloudflare R2** bucket straight after writing.
- **Retention on the 5 GB persistent disk:** newest 20 automatic backups, and pinned backups for
  30 days.
- **Retention in R2** (lifecycle rules): automatic 14 days, pinned 90 days.
- These are app-level uploads, not Litestream's continuous replication, so R2 operation counts
  and cost stay predictable.

**Standby (D7, cloud form):**
- A second Render service, `rfops-standby`, is in the Blueprint and kept suspended.
- A fencing **lease** object in R2 (generation and holder, renewed with conditional PUT) makes
  sure only one service accepts writes. The standby always starts fenced.
- A small **Cloudflare Worker** on `rfops.<domain>` forwards to whichever Render service its
  `ORIGIN` variable names. Changing that variable moves everyone in seconds, with no Render
  domain or certificate change.
- Both services share the session-signing key, so nobody has to sign in again.
- Clients replay commands acknowledged in the last 15 min, which recovers the gap since the last
  backup.
- **Target:** back online in **under 10 minutes**.
- It covers a broken instance, corrupt disk or bad deploy. It does not cover a full Render
  region outage; that falls back to paper.
- Drill it once before the event.

## Security (now on the public internet)

> **Owner decision (Nov 2026): open by default.** `RFOPS_AUTH=open`: no sign-in; each device picks
> a name for attribution, and viewer names are read-only. Anyone with the address can act. Everything
> below is built and tested and applies with `RFOPS_AUTH=pin`. The always-on parts are: the
> agent key, Origin check, headers, the standby lease and backups.

Every requirement is testable. The data structures are in `docs/data-model.md` §2 (operators,
sessions, screens).

1. **Everything requires sign-in:** every page, API call (GET included), photo and WebSocket.
   Exceptions:
   - `/api/health` returns `{ok, version, epoch}` only;
   - the sign-in, join and screen-enrol endpoints;
   - the agent's `/api/midi` (and later telemetry), which uses the agent key.
2. **Operators:**
   - **Every non-helper operator has a PIN** (4–8 digits, argon2). There is no "any name" mode.
   - The **first Lead** is created on first start from `RFOPS_ADMIN_NAME` / `RFOPS_ADMIN_PIN`, or
     the `rfops-server --create-admin` CLI.
   - If a migrated operator has no PIN, every write by that operator is refused until the Lead
     sets one.
3. **Helpers (join codes):**
   - The Lead creates a named helper ("Jo, SL batteries"). Its **6-digit join code** works for
     that helper on **up to 3 devices within 2 h**.
   - The code is shown as a QR plus digits for manual entry. Manual entry matters because iOS
     Home-Screen apps keep their own cookies, separate from Safari: a helper who scanned in
     Safari and then added the icon must type the code again in the app.
   - The kiosk QR is shown only for 60 s after a Lead enters their PIN.
   - A helper who is locked out mid-show (cookie dropped, code used up) is re-admitted with a new
     code from the Lead (Admin → People → Join code).
   - **Re-issue all helper codes** invalidates every outstanding code.
4. **Screens (wall, FOH):**
   - Enrolled by a Lead with a named **screen credential**: 7 days, sliding, revocable, and never
     expiring while a show is in show mode.
   - Actions are attributed "Wall", or "Wall (Sam)" when someone tapped their name chip in the
     last 5 min.
   - Screen roles are a2, foh or viewer, **never Lead**. Lead-only actions need a Lead signed in
     with their own PIN.
5. **Sessions** [have]:
   - **Signed, stateless cookies:** HMAC with `RFOPS_SESSION_KEY`, shared by active and standby.
     They are persistent (`Max-Age`), `HttpOnly; Secure; SameSite=Strict`.
   - 18 h sliding for people, 7 days for screens.
   - A cookie valid when show mode started is accepted until show mode ends, with an absolute
     cap of 7 days from sign-in (screens 30 days).
   - **Revocation:** a list of revoked session ids, plus per-user `sessions_valid_after` (set by
     a PIN change or "sign out everywhere"). Open WebSockets re-check every 10 s.
6. **Rate limits** (checked before any argon2 work; at most 2 argon2 checks run at once) [have]:
   - **Name + device:** 5 wrong PINs in 5 min lock that name on that device for 5 min.
   - **Name, any device:** device ids are chosen by the client, so this limit stops rotation.
     20 wrong PINs in an hour from devices that never signed in as that name pause the name for
     15 min on new devices, and the Lead gets a WARNING.
   - **Trusted devices:** a browser holding a valid `rfops_device` cookie for that name (it signed
     in before) keeps its own budget of 10 per hour. An attacker can't lock the real person out.
   - **Per IP:** a 100 failures / 5 min counter raises a WARNING. It never locks out, because the
     whole crew shares the venue's single public IP.
   - **Join codes:** 6 digits, ≤ 3 uses, 2 h. 5 wrong codes in 5 min per device, plus a
     **global** budget of 20 wrong codes per 5 min, past which joining pauses for 15 min and the
     Lead is warned.
   - **API:** per-session limits.
7. **Roles** are enforced on the server for every mutation, with read scoping:
   - helpers see only their zone tasks;
   - viewers can't see PINs, keys or sessions;
   - `/api/health/detail` and Admin are Lead-only.
8. **Agent key:**
   - Per site and rotatable; **stored hashed**, and the plain key is shown once at creation.
   - HTTPS only.
   - The agent can post MIDI and telemetry only; it cannot read show data.
9. **Transport:** HTTPS only, HSTS, no plain-HTTP endpoints. WebSockets use `wss://`, and the
   server checks `Origin` on the upgrade and on every state-changing request.
10. **Headers:**
    - CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'` (Svelte
      transitions use inline styles); `img-src 'self' blob: data:`; `connect-src 'self'`;
      `frame-ancestors 'none'`.
    - `Referrer-Policy: same-origin`.
    - `Permissions-Policy: camera=(self)`.
11. **Data:**
    - Photos are served only to signed-in users. They are re-encoded server-side, with EXIF and
      location stripped.
    - The R2 bucket is private.
12. **Secrets** (agent key, R2 credentials, session key, VAPID keys) live in Render environment
    variables, never in the repo.
13. **Audit:** sign-ins, failures, role changes, key rotations, enrolments, revocations, restores
    and exports are written to the event log and shown in the report.
14. **Dependencies:** `cargo audit` and `npm audit` in CI; versions pinned.
15. **Privacy:** only names, roles, zones and photos are stored. Deleting an event removes its
    data and, through R2 lifecycle, its backups.

## Costs (estimate; check current pricing)

| Item | Approx. cost |
|---|---|
| Render web service, Starter instance | about $7 / month |
| Render persistent disk, 5 GB | about $1.25 / month |
| Cloudflare R2 | the free tier likely covers it (10 GB, 1 M class-A ops/month) |
| Standby service | suspended; verify Render's charge for a suspended service with a disk |
| Domain | already owned |

## Owner setup steps

To do with the owner's accounts:
1. **Render:** create the services from the repo's `render.yaml` (Blueprint). Auto-deploy off.
2. **Environment variables** (both services):
   - `RFOPS_ADMIN_NAME`, `RFOPS_ADMIN_PIN` (first start only)
   - `RFOPS_SESSION_KEY` (shared)
   - `RFOPS_TZ` (e.g. America/Chicago)
   - `RFOPS_PUBLIC_ORIGIN` (`https://rfops.<domain>`)
   - `RFOPS_INSTANCE` (`active` on `rfops`, `standby` on `rfops-standby`)
   - R2 endpoint, bucket, key and secret
   - VAPID keys
3. **Cloudflare:**
   - Deploy the `rfops-proxy` Worker (`deploy/cloudflare/worker.js`) with a route
     `rfops.<domain>/*` and the variable `ORIGIN=rfops.onrender.com`.
   - The Workers free plan allows 100k requests a day. A show day is far below that (one request
     per API call, one per WebSocket), but check the usage after the first rehearsal.
   - SSL mode **Full (strict)**.
4. **R2:**
   - Create a private bucket, plus an API token scoped to it.
   - Lifecycle rules: `auto/` 14 days, `pinned/` 90 days.
5. **Agent:** set the agent key on the Playback MacBook.
6. **Drills:** run the restore drill and the standby drill once.
