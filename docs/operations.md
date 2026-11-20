# Operations guide

> **Production runs in the cloud** (Render + Cloudflare; `docs/hosting-and-security.md`). This guide
> currently describes running the server on a local Mac. Use that **only for development and
> rehearsal** until the build reaches step 10 (deploy). At that point §1–3 are replaced by the
> Render setup and the screen URLs become `https://rfops.<domain>/…`. The agent (§ bridge)
> setup on the Playback MacBook stays as written.

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

## 5. MIDI from MultiTracks Playback

RF Ops can follow Playback: when a song starts, every screen moves to that song. A small bridge
runs on the Playback Mac and forwards Playback's MIDI cues over the network.

**One-time setup on the Playback Mac** (Rust installed, `rf-ops` cloned):

```bash
cd ~/rf-ops && cargo build --release -p rfops-bridge
```

Get the bridge key from Admin → MIDI → "Show the bridge key" (admins only), then test it by
hand:

```bash
~/rf-ops/target/release/rfops-bridge --server http://192.168.1.10:8080 --key <bridge key>
```

The bridge creates a MIDI destination called **RF Ops**.

- **In Playback:** set **MIDI Out** to **RF Ops**. MIDI Out requires the MultiTracks Cloud
  subscription.
- **In Playback, per song:** add a MIDI cue at the start, Program Change *N* on **channel 16**
  (the default). In RF Ops, Admin → MIDI → **Number 1, 2, 3…** numbers the setlist to match, or
  type a number per song.
- **Check it:** Admin → MIDI shows the bridge as **Online**, and the **MIDI monitor** lists each
  cue and what it did.

Prefer the IAC bus or the PlayAUDIO1U instead? Run `rfops-bridge --list` to see the inputs, then
add `--port "IAC Driver Bus 1"` (or part of the name).

**Start the bridge automatically at login:**

```bash
sed -e "s#RFOPS_HOME#$HOME/rf-ops#g" -e "s#RFOPS_SERVER#http://192.168.1.10:8080#" -e "s#RFOPS_BRIDGE_KEY#<bridge key>#" ~/rf-ops/deploy/macos/com.rfops.bridge.plist > ~/Library/LaunchAgents/com.rfops.bridge.plist
```

```bash
launchctl load -w ~/Library/LaunchAgents/com.rfops.bridge.plist
```

The log is `/tmp/rfops-bridge.log`.

**How it behaves:**

- **Which show follows Playback:** the show in **show mode**, otherwise the latest rehearsal.
- **Follow / Lock:** every song bar has a MIDI status and a **Lock MIDI** button. Locked, cues are
  ignored but logged. ◀ ▶ always work, locked or not.
- **Repeats:** the same cue repeated within half a second counts once.
- **Unknown cues:** a Program Change on the cue channel that matches no song (for example, a
  skipped song) raises a warning and doesn't move anything.
- **Bridge lost:** if the bridge is silent for 10 s during show mode, a "MIDI bridge offline"
  warning appears. It clears when the bridge returns. Meanwhile, advance songs by hand.
- **Optional next/previous notes:** Admin → MIDI can also map a note to "next song" and
  "previous song".

## 6. Before every service

1. Admin → Show & kits: duplicate last week's show, or import the show file, and set the call time.
2. Admin → Assignments: fix any red cells.
3. Backstage: tick the pre-show "Give" handoffs while handing out packs. Log battery checks
   (tap a performer, then Fresh, Checked OK or Replaced).
4. Admin → Show & kits → **Enter show mode…** and go through the checklist. Show mode:
   - locks kits and assignments (swaps, battery checks, alerts and setlist changes still work);
   - takes a backup;
   - raises automatic alerts for packs in use that are unchecked, overdue or low. These clear
     when someone logs a check.

## 7. During the show

- **Mic fails:** tap the alert's **⇄ Swap** (or the performer's card), then **Swap to spare**,
  then tap the spare. Three taps. FOH gets a banner: "Maya now on Ch 9 (was Ch 2)". The failed
  pack is marked faulty unless you untick it.
- **Song changes:** these follow Playback automatically when the bridge is running; ◀ ▶ on any
  screen always work. **Lock MIDI** if Playback goes off-script.
- **Song order changes:** ◀ ▶ on any screen. Skipped songs (Admin → Show & kits → Skip) drop out
  of the handoff list.
- **A screen shows "Offline":** it has stopped receiving updates, and edits are blocked so nothing
  is lost silently. It reconnects by itself. If the server machine went down, restart the service.
  Every screen catches up when it returns.

## 8. After the show

- Admin → Data → **Post-show report**: alerts with response times, swaps, faulty gear, battery
  checks and song changes. Print it or save it as PDF.
- Switch the show back to rehearsal.
- Pull the faulty gear listed in the report.

## 9. Backups and restore

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

## 10. Moving data in and out

Admin → Data:

- **Equipment / performers CSV:** export, edit in a spreadsheet, import. Rows match on label or
  name; any error rejects the whole file, so nothing is half-imported.
- **Mic plot CSV:** who has what in every song.
- **Show file (JSON):** setlist, kits and assignments. Equipment is matched by label, so you can
  move a show between machines with the same inventory.
