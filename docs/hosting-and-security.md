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

## Backups and the "standby"

- **Same as before:** verified backups (`VACUUM INTO` → quick_check → rename) every **2 min in
  show mode** and every 15 min otherwise.
- **New:** each backup is uploaded to a **Cloudflare R2** bucket. Pinned show-start backups are
  kept 90 days; automatic ones 14 days.
  - These are app-level uploads, not Litestream's continuous replication. That keeps R2
    operation counts and cost predictable.
- **Standby (D7, cloud form):** a second Render service, **"rfops-standby"**, is defined in the
  Render Blueprint but kept suspended. The runbook:
  1. Resume `rfops-standby`. It restores the newest verified R2 backup on boot.
  2. Move the custom domain to it in Render (or flip a Cloudflare CNAME).

  Target: back online in **under 10 minutes**, losing at most 2 minutes of changes.
  - It protects against a broken instance, corrupt disk or bad deploy.
  - It doesn't protect against a full Render region outage.
  - Practise it once before the event.

## Security (now on the public internet)

Every requirement is testable.

1. **Everything requires sign-in:** every page, API call (GET included), photo and WebSocket. The
   only exception is `/api/health`, which returns status only and no data.
2. **Sign-in:**
   - Every operator has a PIN (no name-only sign-in).
   - Helpers join with a single-use **join code** that expires.
   - Sessions are **HttpOnly, Secure, SameSite=Strict cookies**, replacing today's header
     tokens. The default lifetime is 18 h; the Lead can revoke them.
3. **Rate limits:**
   - Login: 5 tries per 5 min per IP and per name, then a lockout with backoff.
   - API: per-session limits.
   - Join codes: about 1 million combinations (6 digits), single use, 2-hour expiry.
4. **Roles enforced on the server** for every mutation (as today), and **read scoping**:
   - Viewers can't see PINs, keys or other operators' sessions.
   - Admin endpoints are admin-only.
5. **Agent keys:**
   - The on-site agent uses a per-site key, which can be rotated (as built).
   - The key is sent only over HTTPS.
   - The agent can only post MIDI and telemetry; it can't read show data.
6. **Transport:** HTTPS only, HSTS, no plain-HTTP endpoints. WebSockets use `wss://` and check
   `Origin`.
7. **Headers:**
   - Content-Security-Policy: same-origin scripts only, no inline scripts.
   - `X-Frame-Options: DENY`, `Referrer-Policy: same-origin`, `Permissions-Policy` limited to
     camera (photo capture).
8. **Data:**
   - Performer photos are served only to signed-in users, from the private disk.
   - Uploads are resized and re-encoded server-side, with EXIF/location stripped.
   - R2 bucket private; backup objects are encrypted at rest (R2 default).
9. **Secrets:**
   - The agent key, R2 credentials and session-signing key live in Render environment
     variables, never in the repo or the database.
   - The bridge key (stored in the DB today) moves to a hashed form.
10. **Audit:** sign-ins, failed sign-ins, role changes, key rotations and exports are recorded in
    the event log and the report.
11. **Dependencies:** `cargo audit` and `npm audit` run in CI. Versions are pinned (as now).
12. **Privacy:** only names, roles, zones and photos of performers are stored, and an event can be
    deleted completely (data and backups) after the season.

## What changes in other documents

- **`docs/reliability.md`:**
  - The "server Mac" items (sleep, LaunchDaemon, macOS updates, UPS) apply only to the
    **Playback MacBook running the agent**.
  - Server items become:
    - Render paid instance with persistent disk;
    - deploy freeze;
    - SIGTERM handling;
    - R2 backups;
    - standby service;
    - an external uptime check (for example, Cloudflare health checks or an uptime monitor on
      `/api/health`).
- **`docs/interface-spec.md`:** D5 resolved by real HTTPS; D6 = Render; D7 = cloud standby.
- **`docs/operations.md`:** the install steps become a Render Blueprint (`render.yaml`) plus a
  Cloudflare DNS record. The Mac LaunchAgent for the server is retired. The Mac setup remains for
  the agent.

## Costs (estimate; check current pricing)

| Item | Approx. cost |
|---|---|
| Render web service, Starter instance | about $7 / month |
| Render persistent disk, 1 GB | about $0.25 / month |
| Cloudflare R2 | free tier likely covers it: 10 GB storage, 1 M class-A ops/month. At one upload every 2 min, a 5-hour show is about 150 uploads |
| Domain | already owned |

Standby service: no charge while suspended (verify).

## Owner setup steps

To do when building, with the owner's accounts:
1. Create the Render service from the repo's `render.yaml` (Blueprint).
2. Add the Cloudflare DNS CNAME `rfops.<domain>` → the Render hostname, and add the custom domain
   in Render.
3. Create an R2 bucket and an R2 API token, and put them in Render environment variables.
4. Set the agent key on the Playback MacBook.
5. Run the restore drill once.
