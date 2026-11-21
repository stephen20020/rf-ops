# Installers (stage station and tools)

RF Ops itself runs in the cloud; screens are web browsers. The installers turn a laptop or
touchscreen PC into a **stage station**: the board full screen, never sleeping, reopening itself if
closed, and optionally starting at login. They also include the `rfops` show-file command.

Download them from the GitHub release, or from Actions → Installers → the latest run (artifacts).

## Linux (.deb) — Debian 12+, Ubuntu 22.04+

```bash
sudo apt install ./rfops_0.1.0-1_amd64.deb chromium
```

- **RF Ops Station** appears in the app menu. The first time, it asks for the RF Ops address.
- `rfops-station --url https://rfops.example.com` changes it; `rfops-station --autostart on`
  opens the station when this user logs in.
- `rfops show check|import|export|reset` for show files.
- Optional local server (for rehearsals without internet): `sudo systemctl enable --now rfops-local`
  serves RF Ops at `http://<this machine>:8080`.

For a dedicated station: create a user for it, enable automatic login for that user, run
`rfops-station --autostart on` as that user, and turn off screen locking.

## Windows (x64 .msi) — Windows 10/11

Run `rfops-0.1.0-x64.msi`.

- Start menu → **RF Ops → RF Ops Station** opens the board full screen in Edge (Chrome if no Edge).
  The first time it asks for the address and whether to start at sign-in.
- **RF Ops Station settings** changes the address or start-at-sign-in.
- The screen stays on while the station runs (no admin rights needed for that).
- `rfops` is on the PATH for show files (open a new terminal after installing).

To leave Edge's kiosk mode: Alt+F4 (it reopens after 2 s; close the station from Task Manager →
PowerShell, or sign out).

## Building them yourself

- `.deb`: `cd web && npm run build && cd .. && cargo build --release -p rfops-server -p rfops-cli && cargo deb -p rfops-cli --no-build`
- `.msi`: on Windows with WiX 3.14 — see `.github/workflows/installers.yml`.
