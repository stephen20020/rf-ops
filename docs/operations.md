# Operations guide

How to install RF Ops on the show machine, run it every service, and recover when something
goes wrong.

## 1. Server machine

One computer runs the server. Every screen (the backstage wall, A2 phones and tablets, the FOH
laptop) opens it in a browser. No internet is needed.

- Give the server machine a **fixed IP** on the production network, either a DHCP reservation or
  a static address. The examples below use `192.168.1.10`.
- Put the server machine and the network switch on a **UPS**.
- The network carries Dante too. RF Ops traffic is small, but keep **A2 Wi-Fi on 5 GHz only**:
  the GLX-D guitar packs use 2.4 GHz. Use an access point with multicast filtering or IGMP
  snooping so Dante multicast doesn't flood Wi-Fi. Ideally put Wi-Fi on its own SSID/VLAN,
  routed to the server.

## 2. Build (once, and after each update)

```bash
cd ~/rf-ops/web && npm install && npm run build && cd ..
```

```bash
cargo build --release -p rfops-server
```

## 3. Run as a service on macOS (starts at login, restarts if it stops)

```bash
mkdir -p ~/rf-ops/data
```

```bash
sed "s#RFOPS_HOME#$HOME/rf-ops#g" ~/rf-ops/deploy/macos/com.rfops.server.plist > ~/Library/LaunchAgents/com.rfops.server.plist
```

```bash
launchctl load -w ~/Library/LaunchAgents/com.rfops.server.plist
```

Check it is up: open http://localhost:8080. The log is `~/rf-ops/data/server.log`.

To stop it (for example before restoring a backup):

```bash
launchctl unload ~/Library/LaunchAgents/com.rfops.server.plist
```

Also set **System Settings → Energy → Prevent automatic sleeping** (or the Lock Screen and
Battery equivalents) so the Mac doesn't sleep mid-show. If the firewall is on, allow incoming
connections for `rfops-server`.

**Linux:** copy the repo to `/opt/rf-ops`, create an `rfops` user, then copy
`deploy/linux/rfops.service` to `/etc/systemd/system/` and run
`systemctl enable --now rfops`.

**Windows:** run `target\release\rfops-server.exe` with the same environment variables. To start
it at boot, use Task Scheduler ("At startup", "Run whether user is logged on or not").

## 4. Screens

| Screen | URL | Notes |
|---|---|---|
| Backstage wall touchscreen | `http://192.168.1.10:8080/#/backstage` | Kiosk mode (below) |
| A2 phones / tablets | same URL | "Add to Home Screen" for full screen |
| FOH | `http://192.168.1.10:8080/#/foh` | Laptop or tablet |
| Admin | `http://192.168.1.10:8080/#/admin` | Setup, data, reports |

**Kiosk mode for the wall screen** (Chrome or Chromium, full screen with no browser controls):

macOS:

```bash
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --kiosk --app=http://192.168.1.10:8080/#/backstage --noerrdialogs --disable-session-crashed-bubble
```

Linux / Raspberry Pi:

```bash
chromium --kiosk --app=http://192.168.1.10:8080/#/backstage --noerrdialogs --disable-session-crashed-bubble --overscroll-history-navigation=0
```

Windows: make a shortcut to
`chrome.exe --kiosk --app=http://192.168.1.10:8080/#/backstage`.

iPad: open the URL in Safari, choose Share → Add to Home Screen, then lock it to the app with
**Guided Access** (Settings → Accessibility).

Each screen remembers its operator and show. Set a PIN for each operator (Admin → People), so a
shared screen can't act as someone else.

## 5. Before every service

1. Admin → Show & kits: duplicate last week's show, or import the show file, and set the call time.
2. Admin → Assignments: fix any red cells.
3. Backstage: tick the pre-show "Give" handoffs while handing out packs. Log battery checks
   (tap a performer, then Fresh, Checked OK or Replaced).
4. Admin → Show & kits → **Enter show mode…** and go through the checklist. Show mode:
   - locks kits and assignments (swaps, battery checks, alerts and setlist changes still work);
   - takes a backup;
   - raises automatic alerts for packs in use that are unchecked, overdue or low. These clear
     when someone logs a check.

## 6. During the show

- **Mic fails:** tap the alert's **⇄ Swap** (or the performer's card), then **Swap to spare**,
  then tap the spare. Three taps. FOH gets a banner: "Maya now on Ch 9 (was Ch 2)". The failed
  pack is marked faulty unless you untick it.
- **Song order changes:** ◀ ▶ on any screen. Skipped songs (Admin → Show & kits → Skip) drop out
  of the handoff list.
- **A screen shows "Offline":** it has stopped receiving updates, and edits are blocked so nothing
  is lost silently. It reconnects by itself. If the server machine went down, restart the service.
  Every screen catches up when it returns.

## 7. After the show

- Admin → Data → **Post-show report**: alerts with response times, swaps, faulty gear, battery
  checks and song changes. Print it or save it as PDF.
- Switch the show back to rehearsal.
- Pull the faulty gear listed in the report.

## 8. Backups and restore

Backups are written to `data/backups/` (next to the database):

- every 15 minutes when something changed;
- when a show enters show mode;
- when you press Admin → Data → Back up now.

The 40 most recent are kept.

To restore:

```bash
launchctl unload ~/Library/LaunchAgents/com.rfops.server.plist
```

```bash
cp ~/rf-ops/data/backups/<backup-file>.db ~/rf-ops/data/rfops.db && rm -f ~/rf-ops/data/rfops.db-wal ~/rf-ops/data/rfops.db-shm
```

```bash
launchctl load -w ~/Library/LaunchAgents/com.rfops.server.plist
```

Also copy `data/backups/` to another drive now and then.

## 9. Moving data in and out

Admin → Data:

- **Equipment / performers CSV:** export, edit in a spreadsheet, import. Rows match on label or
  name; any error rejects the whole file, so nothing is half-imported.
- **Mic plot CSV:** who has what in every song.
- **Show file (JSON):** setlist, kits and assignments. Equipment is matched by label, so you can
  move a show between machines with the same inventory.
