# RF Ops interface spec (v1, before Phase 5)

**Purpose.** This spec defines every screen, control, state and rule before Phase 5 adds live
Shure telemetry. Telemetry multiplies data and alerts, so the screens need to be right first. It
is built from three research tracks (A2 workflow practice, comparable products, alarm/HMI
standards; sources at the end) and an audit of the current build (`docs/ux-audit.md`).

**Scope.** All four surfaces (wall, phone, FOH, admin), the alert system, battery handling,
handoffs and checklists, swaps, chat, setup and post-show. Each section ends with acceptance
tests.

## Decisions confirmed by the owner (Nov 2026)

1. **Wall board = one tile per device** in fixed positions, showing the current holder (§5).
2. **FOH must confirm every swap** ("Done at console") (§9).
3. **Quick messages: canned phrases only**, with a device reference and an urgent flash (§12).
4. **Performer photos** via phone-camera upload in Admin → People, shown on tiles, handoff cards
   and FOH rows (§13.4).

---

## 0. Ten principles (everything else follows from these)

1. **Hardware is the anchor; people move.** Every problem, battery and swap belongs to a *device*
   (Red, Bpack2, RX10), and shows the person holding it *now*. Crews say "Red's down", not
   "Helen's down". This fixes audit bug 2.
2. **Stable positions.** A tile never moves during a show. Order is fixed by the mic plot:
   colour mics in fader order, then bodypacks, then IEM packs by RX number. Off-stage tiles dim in
   place. (Audit risk 3; WWB dims offline strips rather than hiding them.)
3. **Grey when healthy, colour only when something needs action.** A healthy show screenshot
   contains no red or amber. Mic identity colours (Yellow, Red…) appear only as a small chip, never
   as status. (ISA-101; 14 CFR 25.1322(f).)
4. **Three priorities, each with a defined response.**
   - **CRITICAL:** act now.
   - **WARNING:** act before the next song.
   - **NOTE:** awareness only; never interrupts.

   An alert that needs no action is not an alert. (ISA-18.2; FAA warning/caution/advisory.)
5. **Never show "OK" without evidence.** Every value has an age. Unknown, stale and "can't know"
   (one-way IEM packs) each look different from healthy. (Micboard PREV/unknown states; crew
   complaint "false confidence".)
6. **Never colour alone.** Every state has a word plus a shape. Battery has a number, and
   minutes when known. (WCAG 1.4.1; about 1 in 12 men are colour-blind.)
7. **Three taps for anything urgent; zero typing during a show.** Notes are added after; time and
   song are captured automatically. (Crew practice; Shure volunteer guidance "≤3 steps".)
8. **Undo, don't confirm.** Reversible actions apply instantly with a 10 s undo. The only
   confirmation is the final swap button, labelled with the exact action.
   (NN/g; WCAG 3.3.4.)
9. **Nothing covers an alert.** No modal hides the alert strip; no toast covers content.
   (ISA-101 situational awareness.)
10. **Same truth everywhere, shaped per role.** One data model; the wall shows the overview,
    phones show "my next task", FOH shows channels, admin edits the plot.

---

## 1. Roles, devices, and what each sees first

| Role | Device | First screen ("level 1") | Can do |
|---|---|---|---|
| A2 (backstage) | Wall touchscreen (24–32″, 1080p, 1–3 m) | **Board**: all devices as tiles plus the next-handoff strip | Everything operational |
| A2 (roaming) | Phone | **My next**: my next handoffs, my alerts, a "problem" button | Everything operational, filtered to *mine* |
| FOH engineer | Laptop or iPad beside the console | **Channels**: one row per console channel/IEM pack, plus now/next | Report problems by channel, ack swaps, song control, chat |
| Lead / admin | Laptop | **Plot**: mics × songs grid | Edit everything; show mode |
| Viewer (MD, stage manager) | Any | Board, read-only | Nothing |

- **Remembered per device:** the role, operator and screen are chosen once and stored.
- **Kiosk URL:** `/#/board?kiosk=1` hides navigation, disables zoom, selection and pull-to-refresh,
  and returns to the board after 60 s idle.
- **Joining:** a QR code on the board's corner menu opens the same server on a phone (Micboard
  pattern).

---

## 2. Vocabulary (the UI uses crew words)

| Concept | UI word | Never |
|---|---|---|
| Handheld/bodypack transmitter | **mic** (Red, Bpack2) | "carrier", "kit" |
| IEM bodypack | **pack** (RX10) | "IEM kit" |
| IEM feed | **mix** (ME 3, Console) | — |
| Receiver channel | **channel** (Ch 34) | "base" |
| Mic + capsule + channel bundle | (hidden: always shown as the mic name) | "kit" (admin only) |
| Change between songs | **handoff**: *give*, *collect*, *pass*, *capsule swap* | "transition" |
| Failed device replaced | **swap to spare** / **on spare** | — |
| Device out of use | **out of service** | "faulty" (admin only) |
| Alert states | **NEW → SEEN → FIXED** | "acknowledged/resolved" (ack and resolve remain the API terms) |
| Battery check | **batt OK / fresh batts / LOW** | "BatteryResult" |

---

## 3. Shared chrome (every screen)

### 3.1 Status bar (top, always visible, 56 px; 72 px on the wall)
`[Show name ▾]  [● LIVE / ● OFFLINE 0:42]  [SHOW MODE]  [MIDI: Following ✓ / Locked 🔒 / Bridge offline]   [⚠ 1 CRITICAL] [▲ 3 WARNING] [i 2]   [Operator ▾]`

- **Master alert counts** (FAA master caution) are always there. Tapping one opens the alert
  list filtered to that priority. A critical count has a filled red background *and* an octagon
  glyph; a warning has an amber outline *and* a triangle.
- **OFFLINE:** a full-width banner below the bar: "OFFLINE — showing data from 20:14:05 (0:42 ago).
  Changes are paused." The whole board desaturates and gets a hatched overlay. Telemetry values
  turn to "—".
- **Show name** never truncates the important part: it shortens from the left, so "…Afternoon —
  Saturday" stays readable.

### 3.2 Song strip (below the status bar)
`[◀]  NOW 2/3 · I Will Follow Jesus / Lace Up Your Boots   NEXT: Concert (after handoff)   [▶]`

- **Done songs** are greyed in the drop-down setlist (tap the title to jump; asks "Jump to …?"
  with an undo).
- **Wrapping:** the title wraps to two lines on a phone rather than truncating. The next-song
  title may truncate.
- **MIDI-driven changes** show a small "via Playback" for 5 s.

### 3.3 Toasts → **activity line**
- **Replaces toasts.** A single-line ticker under the song strip shows the last event ("Sam
  marked Red batt OK · 4 s"). It's dismissable, never overlaps content, and keeps a 10 s
  **Undo** where possible.
- **Critical events don't use the activity line.** They arrive in the alert strip.

### 3.4 Alert strip
- **Wall and FOH:** a right-hand column, critical at the top.
- **Phone:** pinned at the top, collapsible to one line ("1 CRITICAL · Red no audio").
- **Never covered.** Sheets open beside it or below it.

---

## 4. Device tile (the core component)

Used on the board (wall, phone list) and in the swap sheet.

```
┌───────────────────────────────────┐
│▌RED   Ch 34                  [▲ LOW]│  ← mic chip + channel; status word+glyph right
│  [photo] Helen                     │  ← holder now (large)
│          Vocal · SL                │  ← role · zone (small)
│  ▮▮▮▯▯  2:10  ·  RF ▮▮▮▮  ·  A     │  ← telemetry row (Phase 5); manual batt otherwise
│  NEXT → Shosh (pass before song 3) │  ← only if it changes next song
└───────────────────────────────────┘
```

### 4.1 Tile states (one state per tile)

| State | When | Look (shape + word + colour) |
|---|---|---|
| **In use** | Assigned in the current song, healthy | Neutral grey border, white name |
| **Standby** | Not in the current song, but assigned later | Dimmed 60%, "NEXT: Song 3 →" line |
| **Idle** | Not used again this show | Dimmed 35%, "idle" |
| **Spare** | Spare, ready | Dashed border, "SPARE" tag, "~" before the name when live (TheatreMix) |
| **On spare** | Performer moved to this spare | Normal plus an "ON SPARE (was Red)" tag |
| **Out of service** | Marked faulty | Diagonal hatching, "OUT", greyed; stays in place |
| **Warning** | Active warning on this device | Amber 3 px border plus ▲ and a word ("LOW", "CHECK BATT") |
| **Critical** | Active critical | Red 6 px border, filled red header, ⬣ plus a word ("NO AUDIO", "RF LOST"). Pulses until SEEN |
| **No data** (Phase 5) | Receiver hasn't reported in 30 s | Telemetry row hatched, "no data 0:45" |
| **Can't report** | One-way IEM packs | Battery shows the manual check age only, "manual" |

### 4.2 Battery display (all devices)
- **ULX-D telemetry (Phase 5):** 5 segments plus `2:10` (h:mm) on SB900. On AA, segments only.
  "≈" if older than 60 s.
- **Manual (IEM, GLX-D, or before Phase 5):** "✓ 18:42 fresh (Sam)", or nothing until due.
- **Due model:**
  - Before **call time**, nothing is shown.
  - From call time until checked: "CHECK BATT" as a warning, **but only once the show is in show
    mode or call time has passed**.
  - Once checked: a quiet "✓ 18:42".
  - LOW: a warning, or critical if the device is in use now.
- **Bulk check:** a pre-show "battery service" checklist (§8) records many devices at once.

### 4.3 Tap targets on a tile
- **Tap:** opens the **device sheet** (§6).
- **Long-press:** quick menu: Batt OK · Fresh batts · LOW · Problem · Swap.
- **No other gestures.**

---

## 5. Wall board (backstage touchscreen)

Layout at 1920×1080:

```
[status bar 72px]
[song strip 96px]
[activity line 40px]
┌──────── Board (grid, 6 cols) ────────┐┌─ Alerts ──────┐
│ MICS:  Yellow Red Blue Green Black    ││ ⬣ Red NO AUDIO │
│        White Orange Tan               ││   [SWAP][SEEN] │
│ PACKS: Bpack1-4                       ││ ▲ 3 batt checks│
│ IEM:   RX1…RX10, Gabe's, Senn 1/2     │├─ Next handoffs ┤
│                                       ││ before Song 3   │
│                                       ││ PASS RX10 Shosh→Helen │
└───────────────────────────────────────┘└────────────────┘
```

- **Grid:** fixed device order. 25 devices fit a 6×5 grid at about 290 px wide.
  - Section labels "MICS", "BODYPACKS", "IEM".
  - Unassigned devices still show (dimmed, "unassigned"). The board doubles as the
    mic-drawer/rack picture.
- **Type sizes** (2 m viewing; research rule 17; ~0.277 mm/px on a 24″ 1080p panel):
  - Holder name ≥ 40 px.
  - Mic ID ≥ 32 px bold.
  - Status word ≥ 28 px.
  - Secondary ≥ 22 px.
  - Alert titles ≥ 36 px.
  - A **Size** setting (S/M/L) per device covers 1 m vs 3 m mounting.
- **Next-handoffs panel:** only the next transition, plus a collapsed preview of the one after.
  Sorted PASS / CAPSULE SWAP first, then GIVE / COLLECT grouped by person. Each row is a 64 px
  target. Ticking it records the handoff *and*, for GIVE and COLLECT, the checkout and return.
- **Filters:** "All · Mine · SL · SR · IEM only" chips (zones from the people list).
- **Dark theme:**
  - Background #121418 (not pure black); body text #C9D1D9 (not pure white).
  - Brightness/"dim" toggle in the corner menu. "Dim" lowers the whole UI to 60%, with alerts
    staying full brightness.
  - A light theme for daytime load-in.

**Acceptance:**
- With the HF Saturday data, every device fits without scrolling at 1080p.
- No tile moves across a song change.
- A critical alert is readable from 3 m (measured).
- The pre-show board with no checks due shows zero amber or red.

---

## 6. Device sheet (tap a tile)

This opens as a side panel on the wall and iPad (the alert column stays visible) and as a
bottom sheet on the phone. It is never centred over alerts.

1. **Header:** mic chip + name + channel/mix + holder photo and name + "next: …". For IEM:
   "Pack RX10 · mix ME 3 · TX10 (also feeds: —)".
2. **Big actions** (64 px):
   - **Batt OK** (primary, most common), **Fresh batts**, **LOW**.
   - **Problem…**, which opens the preset list: No audio · Crackle · Dropout · Low RF · Wrong pack ·
     Sweat · Cable · Other.
   - **Swap to spare**.
3. **This show:** the holder for each song (mini plot row), with the current song highlighted.
4. **History:** battery checks, problems and swaps, with who and when.
5. **Out of service / Back in service** (lead only; asks for a reason).

**Acceptance:**
- Batt OK takes 2 taps from the board (tap tile, tap Batt OK). With long-press it's 1.
- Every action shows in the activity line with Undo for 10 s.

---

## 7. Problems, alerts and the alert list

### 7.1 Raising a problem: 2 taps from any screen
- **From a tile or FOH row:** Problem → preset. The device, current holder, song and time come
  from context. No severity choice: each preset has a default.
  - No audio, Dropout and RF hit: CRITICAL if in use now.
  - Crackle, Wrong pack and Cable: WARNING.
  - Sweat and Low batt: WARNING.
  - Other opens an optional text box.
- **"Problem" button with no device (phone, FOH):** a big grid of device chips; pick one, then a
  preset.
- **Alerts attach to the device;** the holder is derived. If the device changes hands, the alert
  follows the device and shows "(was Helen)".

### 7.2 Alert lifecycle and display
- **NEW**: flashing glyph, top of the list, master count.
- **SEEN**: someone took it; shows "Sam is on it · 0:12". Flashing stops.
- **FIXED**: removed from the list, kept in the log.
- **Auto-fixed** (telemetry or rule cleared): shown as "cleared" for 30 s, then logged.
- **Each alert row:** priority glyph + word, device chip + holder, problem, age, who raised it.
  - Actions: **Swap** (if the device has a spare of its kind), **On it** (= SEEN), **Fixed**.
  - The fix note is optional and can be added later from the log.
- **Grouping** (research rule 4):
  - The wall shows at most 5 individual alerts, plus "+N warnings".
  - Rule alerts of one kind are grouped ("4 packs need a battery check →").
  - Critical alerts are never grouped.
- **Suppressed by design** (rule 7): devices idle for the rest of the show never raise warnings
  or criticals; any rule finding goes in as NOTE.
- **Snooze** (= ISA shelving): a WARNING can be snoozed "until next song" or for 15 min. It
  shows as a "snoozed" count and comes back automatically. CRITICAL can't be snoozed.
- **Duplicate guard:** reporting the same problem on the same device within 2 min adds "+1 (FOH)"
  to the existing alert instead of creating a new one.

### 7.3 Sound (optional, per device, off by default)
- **One tone for new criticals only,** none at FOH unless enabled. Phones vibrate for criticals
  on *my* devices.

### 7.4 Flood mode
- **Trigger:** more than 10 non-NOTE alerts in 10 minutes.
- **Effect:** a banner reads "Many alerts — showing first cause first". The list switches to
  first-out ordering, grouped by device.

**Acceptance:**
- Raising "No audio" on Red from the FOH channel row takes 2 taps.
- The alert appears on all screens within 1 s, on Red's tile and on the holder's phone.
- An alert on RX10 raised while Helen holds it moves with RX10 to Shosh.
- In show mode with 20 unchecked packs, the wall shows **one** grouped row, not 20.

---

## 8. Checklists (pre-show, post-show)

DO-CONFIRM pause points with at most 9 items each (Gawande; WHO; Degani & Wiener). Each is
reached from the status bar ("Pre-show 6/9").

### 8.1 Battery service (a checklist screen, wall and phone)
- **Layout:** a grid of all devices in board order. Each cell is a big toggle that cycles through
  ✓ fresh / ✓ OK / LOW / —.
- **Shortcut:** **"All fresh"** marks every device in use in the show. Cells can be corrected
  individually.
- **Logging:** each check records who and when.
- **Default timing:** call time = concert start − 1 h, per the Homestead sheet.

### 8.2 Pre-show checklist (per show; editable list; defaults)
1. Batteries serviced (from §8.1; auto-ticks when every in-use device is ✓).
2. Every mic talked into: line check (per-device tick on the board, or a bulk tick).
3. IEM packs on the right channel, volume set, limiter on.
4. Spares batteried, tested and staged (shows spare count).
5. A2 Wi-Fi on 5 GHz only.
6. dLive scene/patch verified, and FOH confirms the mic order.
7. MIDI bridge online (auto-ticks).
8. No assignment conflicts (auto).
9. Choir mic and choir ME-1 connections checked.

**Enter show mode** offers the checklist. Open items are listed with "Enter anyway".

### 8.3 Post-show
1. Count-in: tick every device collected. COLLECT handoffs auto-tick.
2. Batteries out / rechargeables on charge.
3. Wipe handhelds and packs.
4. Review faults: the out-of-service list plus fix notes.
5. Report: print or save.

**Acceptance:**
- Pre-show battery service for 25 devices takes ≤ 30 s with "All fresh" plus 2 corrections.

---

## 9. Swap to spare

1. **Start:** from an alert (**Swap**) or a device sheet (**Swap to spare**). That is 1 tap.
2. **Swap sheet** (side panel):
   - Spares of the same type, best first: tested, batt ✓, on *different receiver hardware*
     (PSW "main and spare on separate units"), not in use.
   - Each spare is a 72 px card: chip, channel, batt, "staged SL".
   - Toggles: **Rest of show** / **This song**, and **Mark Red out of service** (on).
3. **Tap the spare card:** this is the swap (tap 3). The button text is the action:
   "Swap Helen: Red → Spare A (Ch 42)".
4. **Result:**
   - The board updates immediately, with "ON SPARE (was Red)".
   - The alert is SEEN.
   - The activity line has Undo.
   - **FOH gets a SWAP card that must be confirmed:** "Helen now on **Ch 42 (Spare A)** — was
     Ch 34. [Done at console]". It stays until FOH taps Done, and backstage sees "FOH: waiting…"
     then "FOH ✓ 0:08". This prevents the "other console not patched" failure (PSW).
5. **Back to main:** when Red is back in service, the device sheet offers "Return Helen to Red".

**Acceptance:**
- 3 taps from the alert.
- The FOH card appears in < 1 s.
- Backstage sees the FOH confirmation.
- The undo within 10 s restores everything.

---

## 10. Phone: "My next" (roaming A2)

Bottom tab bar (thumb zone): **Next · Board · Alerts · Check**.

- **Next tab** (default):
  - My next handoffs as big cards: "PASS RX10 Helen → Shosh · before *I Will Follow Jesus* ·
    SL". Each has **Done** (64 px) and **Problem**.
  - Sorted by when they happen. Finished cards fold away.
  - Countdown to the song change when MIDI timing is available (future).
- **Board tab:** the device tiles as a 2-column list, filter chips at the top.
- **Alerts tab:** the full list. A floating **Problem** button sits bottom-right on every tab.
- **Check tab:** battery service (§8.1) and checklists.
- **Song strip:** reduced to one line with ◀ ▶ (◀ ▶ hidden for viewers).

**Acceptance:**
- At 390×844 with HF data, the first handoff card is visible without scrolling.
- Done is a single tap.
- The title never truncates to the point of being ambiguous (two lines allowed).

---

## 11. FOH: Channels

```
[status bar][song strip]
┌ Channels ─────────────────────────────────────────────┐┌ Alerts / Swaps ┐
│ Ch  Mic     NOW          NEXT        BATT   STATUS  [!] ││ SWAP: Helen →  │
│ 33  ▌Yellow Judah        Judah       ✓      —       [!] ││ Ch 42 [Done]   │
│ 34  ▌Red    Helen        (off)       LOW ▲  —       [!] ││ ⬣ Red NO AUDIO │
│ …                                                       │└────────────────┘
│ IEM pack  Mix   NOW      NEXT     BATT                 [!]│
│ RX10      ME 3  Shosh    Helen↩   ✓ 18:40               │
└──────────────────────────────────────────────────────────┘
```

- **Rows** are 48 px. Each row's **[!]** opens the 2-tap problem presets for that device.
- **The NEXT column** highlights changes (TheatreMix delta idea): "→ Shosh", "off", "↩ returns".
- **IEM order:** pack RX number, then other packs (not mix text).
- **Battery column only;** there's no separate status column. Status shows only alerts.
- **Swap cards** that need a FOH tap (§9) sit at the top of the right column.
- **Quiet by default:** no sound and no motion except a NEW critical. A dark, low-glare variant.

**Acceptance:** at 1440×900 all 12 mic rows plus 13 IEM rows are visible with one scroll at most.

---

## 12. Quick messages (FOH ↔ backstage, optional but recommended)

- **Why:** this keeps mic chatter off comms (WAVETOOL, PSW).
- **Canned phrases:** "Check Red", "Red is hitting", "Go to spare", "Copy", "Standing by",
  "Pull Red after this song", "Thanks".
- **Device reference:** tap a device chip to attach it.
- **Display:** messages appear in the activity line and in a small message log. **Urgent**
  (long-press Send) flashes the receiving screens' alert strip once.
- **No free typing** in show mode (lead can).

---

## 13. Admin

### 13.1 Plot (replaces the Assignments grid)
- **Mic plot orientation:** rows = devices in board order (Yellow … Bpack4, RX1 … RX10,
  others); columns = Default + each song (done songs greyed).
- **Cells:** the person's name. Tap a cell to get a people picker (search plus recent).
  "↳" means the default applies; "—" means off.
  - **Conflicts:** a red cell, with the reason on hover or long-press.
- **Drag a name** along a row to fill songs. "Copy column", "Clear column".
- **Person view toggle:** the person-rows grid that exists today (useful for "what does Helen
  have").
- **IEM loans:** "Lend for this song" on a cell creates the borrow and the automatic return.
- **Locked in show mode** (as now), with a banner.

### 13.2 Devices (merges Inventory + kits)
- **One table:** device, type, channel/receiver, capsule/headset, console ch, mix, spare flag,
  status, notes, current holder. Inline editing (fixes "kits can't be edited").
- **Bulk add:** "Add 8 handhelds named …". CSV import/export stays.
- **Receivers** (Phase 5) get a separate sub-table with IP, firmware and online state.

### 13.3 Show
- Name, date, **concert time** and **call time** (call defaults to concert − 1 h), mode buttons,
  and the checklist editor.
- **Setlist:** a paste box (one song per line) as well as one-by-one entry. Drag to reorder, skip,
  and PC number. "Number 1, 2, 3…".
- Duplicate as template / import / export.

### 13.4 People
- Name, role, zone (choose from **SL / SR / Center / Riser / Pit / FOH**, editable list), A2 owner,
  **photo** (upload, or take one with the phone camera), and notes.
- **Bulk "assign to A2"** by multi-select or zone ("All SL → A2 Wireless").

### 13.5 Operators, MIDI, Data
- As built, plus a QR code to join a device.

---

## 14. Data model changes this spec needs

| Change | Why |
|---|---|
| `alerts.equipment_id` required for device problems; holder derived at read time | Alerts follow hardware (P1) |
| `alerts.state` gets `snoozed_until`; `alerts.repeat_count` | Snooze; duplicate guard |
| `swaps.foh_confirmed_at/by` | FOH confirmation |
| `performers.photo` (file in data dir), `zones` list | Photos; filters |
| `shows.concert_time` | Call time default; battery due |
| `checklists` (per show: items, ticks with who/when) | Pre/post checklists |
| `messages` (canned, device ref, urgent) | Quick messages |
| Handoff tick creates a checkout or return for GIVE/COLLECT | Unifies the two (audit 10) |
| Device sort order (`equipment.board_order`) | Stable board |
| Events replayed on connect are marked `replay: true`; clients never notify on replay | Audit bug 1 |

---

## 15. Visual system

- **Neutrals:** bg #121418 · panel #1A1E24 · line #2A3038 · text #C9D1D9 · muted #8B949E.
- **Status:**
  - CRITICAL #E5484D, filled, with an octagon glyph.
  - WARNING #F5A524, outline plus triangle.
  - NOTE #7AA2F7 with a circle "i".
  - OK: no colour, an optional ✓ glyph.
  - Status pairs differ by ≥ 3:1 luminance.
- **Device identity chips:**
  - Yellow, Red, Blue, Green, Black, White, Orange, Tan as a 10 px left stripe plus the word.
  - Red and Orange chips are paired with the word so they can't be confused with status.
- **Patterns:** hatching = out of service / no data; dashed border = spare.
- **Type:** system UI font, tabular numerals for times and channels.
- **Motion:** only a NEW critical pulses (1 Hz, stops when SEEN); `prefers-reduced-motion` turns it
  into a static double border.
- **Colour-blind check:** every screen is verified in greyscale and in deuteranopia and protanopia
  simulation (tests, §16).

---

## 16. Test plan (acceptance, automated where possible)

1. **Visual regression** of each surface at wall, iPad, FOH and phone sizes in four states: pre-show,
   in show healthy, critical + swap, offline.
2. **"No colour when healthy":** a screenshot pixel scan finds no status red or amber in the
   healthy state.
3. **Colour-blind:** the same screenshots through deuteranopia and protanopia filters keep every
   state distinguishable by glyph and word (manual review checklist).
4. **Stable layout:** tile bounding boxes are identical across every song change.
5. **Speed:** raise-to-everywhere < 1 s; swap-to-FOH card < 1 s; Batt OK 1–2 taps; swap 3 taps.
6. **Replay:** a fresh page load never toasts, vibrates or flashes for old events.
7. **Flood:** 25 rule alerts produce ≤ 5 rows on the wall.
8. **Touch:** every primary target ≥ 64 px on the wall, ≥ 44 px on the phone.
9. **Volunteer test** (manual): a new person does battery service, a handoff and a swap from a
   one-page card without help.

---

## 17. Build order (one pass, no rework)

1. **Data model** (§14) plus the replay fix plus alerts-follow-device. Server tests.
2. **Shared components:** status bar, song strip, activity line, alert strip, device tile, device
   sheet, problem presets.
3. **Wall board** plus next-handoffs panel plus battery service.
4. **FOH channels** plus swap confirmation cards.
5. **Phone tabs.**
6. **Admin:** plot grid, devices table, show/setlist paste, people (photos, zones, bulk A2).
7. **Checklists, quick messages, report additions** (open faults at the top).
8. **Full test plan** (§16) plus screenshots for review.

Then Phase 5 drops telemetry into the tile's telemetry row and the rule engine without touching
layout.

---

## Sources (selected)

- **Alarm management:**
  - ISA-18.2 / EEMUA 191 rates and priority distribution, via Emerson "Alarm management by
    numbers" and the ASM Consortium/Honeywell benchmarking paper.
  - Joint Commission Sentinel Event Alert 50; Bonafide et al., J Hosp Med 2015 (alarm fatigue
    and response time).
  - 14 CFR 25.1322 and AC 25.1322-1 (warning/caution/advisory; colour reservation).
- **HMI:** ISA-101 / High Performance HMI ("Going Gray", control.com; ISA blog).
- **Legibility:** MIL-STD-411F, FAA HFDS ch. 5, FHWA (visual angle); ADA 703.5; WCAG 2.2
  1.4.1/1.4.3/1.4.6/1.4.11/2.5.2/2.5.5/2.5.8/3.3.4.
- **Touch targets:** Parhi, Karlson & Bederson, MobileHCI 2006.
- **Checklists:** Gawande, *The Checklist Manifesto*; WHO Surgical Safety Checklist (Haynes,
  NEJM 2009); Degani & Wiener, NASA checklist design.
- **A2 practice:**
  - ProSoundWeb: "Eyes on the Mics", "Brother, can you spare a spare", "Wireless on the road".
  - Shure insights: WAVETOOL theatre case studies, "Keeping it simple for the volunteer audio
    team".
  - Theatrecrafts sound paperwork and calls/cans; Clear-Com intercom etiquette.
  - RF Venue IEM guide; Broadway Green (WICKED rechargeables); ULX-D manual (SB900 runtime
    ±15 min).
- **Products:**
  - Micboard source (karlcswanson/micboard: state model, PREV states, 30-min unknown, stripes,
    TV mode, QR).
  - WAVETOOL 3 help and WAVETOOL 4 pages (players, alerts reset, macros/chat, snapshots by MIDI).
  - Wireless Workbench 7 help (monitor tab, dimmed offline, latched overload, interference
    levels, monitor-only mode).
  - MicTrac (base assignment + slot overrides, ready toggle, viewer link).
  - TheatreMix features (cue list, change colours, backup "~", colour-blind palette).
  - Sennheiser Control Cockpit manual (status bar colours, battery time buckets).
