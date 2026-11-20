# RF Ops interface spec — v2

**Status:** v2 (Nov 2026). Supersedes v1.

**Builds on:**
- the build audit (`docs/ux-audit.md`);
- four research tracks: A2 practice, comparable products, alarm/HMI standards, reliability;
- primary-source hardware facts (`docs/hardware-integration.md`);
- an independent review of v1. Its 26 findings are each resolved in §21.

**Companion documents:**
- `docs/reliability.md`: failure modes, 26 reliability requirements, show-day setup.
- `docs/hardware-integration.md`: device facts and bench tests.
- `docs/hosting-and-security.md`: Render hosting, backups, standby and security requirements.

## Owner decisions

| # | Decision | Status |
|---|---|---|
| D1 | Wall board = one tile per device, fixed positions | Confirmed |
| D2 | FOH must confirm every swap | Confirmed |
| D3 | Quick messages, canned phrases only | Confirmed |
| D4 | Performer photos (phone camera upload) | Confirmed |
| D5 | HTTPS everywhere | **Resolved:** real certificate via Render on the owner's domain. Phone notifications, keep-awake and the offline shell are available |
| D6 | Hosting | **Cloud only on Render** (paid instance with persistent disk), DNS and R2 backups on Cloudflare. Venue has reliable wired internet; internet loss = paper fallback (`docs/hosting-and-security.md`) |
| D7 | Standby | **Cloud standby:** a suspended second Render service restores the newest R2 backup; target < 10 min |
| D8 | Spare mics | **No dedicated spare.** Swap offers mics free in the current song (§10.2), then fallback options |

---

## 0. Principles

1. **Hardware is the anchor; people move.** Problems, batteries and swaps belong to a device (Red,
   Bpack2, RX10), shown with whoever holds it *now*. The holder at the moment an alert was raised
   is stored on the alert.
2. **Stable positions.** Tiles never move during a show. Not-in-use tiles dim their *background*,
   never their text.
3. **Grey when healthy.** Status colour appears only when something needs action.
   - Device identity colours (the Yellow… chips) are a separate visual channel: a left stripe plus
     the word.
   - Chips are excluded from the "no colour when healthy" test.
4. **Three priorities, each with a defined response:**
   - **CRITICAL**: act now.
   - **WARNING**: act before the next song.
   - **NOTE**: awareness, never interrupts.

   One table (§7.6) defines every alert type's priority. Nothing is decided ad hoc.
5. **Never imply OK without evidence.** Every device always shows its battery evidence:
   - "not checked yet";
   - "✓ 18:42 by Sam" (with the date if it isn't today);
   - or live telemetry.

   "Manual" devices say so. "Unknown" is never shown as healthy.
6. **Never colour alone.** Every status has a word and a glyph. Battery shows a number, and
   minutes when known.
7. **Visible controls first.** Every action has a visible button. Long-press and drag are
   shortcuts only.
8. **Confirmation rule.** Reversible actions apply instantly with a 10 s Undo. Confirmation is
   only for:
   - (a) the swap button, which is labelled with the exact action;
   - (b) a song jump of more than one;
   - (c) changing the active show;
   - (d) marking a device out of service.
9. **Nothing covers an alert.** No modal over the alert column. No toasts; an activity line
   replaces them.
10. **Same truth, shaped per role.** One model. The wall shows the overview, phones show "my next",
    FOH shows channels, admin edits the plot.
11. **Honest when offline.** A stale screen is unmistakable. Low-risk taps queue visibly, and
    everything else is refused clearly.

---

## 1. Roles, devices, onboarding

| Role | Typical device | Default screen | Permissions |
|---|---|---|---|
| **Lead** (admin) | Laptop / any | Plot | Everything, including show-mode overrides |
| **A2** | Wall touchscreen, phone | Board (wall) / My next (phone) | All live operations; song control: hold-to-advance on phones |
| **FOH** | Laptop/iPad at the console | Channels | Report, confirm swaps, song control, messages |
| **Helper** (volunteer A2) | Phone | My next | Handoff Done, battery checks, report Problem; no swaps, no song control |
| **Viewer** | Any | Board | Read only |

### 1.1 Joining and onboarding

- **Join code.** The Lead taps **Add helper**, types a name, picks a zone (SL/SR/…), and taps
  Add. RF Ops shows a QR code plus a 4-digit join code. The helper scans it and is signed in
  with their zone's performers already as "mine". There is no separate PIN entry for helpers;
  the join code expires after the event.
- **Assigning devices.** "Mine" for a phone = performers whose A2 owner is this operator, plus
  their zone. The Lead assigns zones in bulk (§15.4).
- **First open.** A first-time phone shows a **3-card intro**: "Your next jobs are here",
  "Tap Done when handed over", "Tap Problem if anything is wrong". Then it goes to My next.
- **Practice show.** **Practice show** (from Admin) clones the active show into a sandbox marked
  PRACTICE. A diagonal "PRACTICE" watermark covers every screen in the sandbox. It is used for
  training and never affects the real show.

### 1.2 Active show (server-wide)

- **One active show.** The server holds one **active show**. Every screen follows it unless a
  Lead pins a screen to another show (admin editing).
- **Changing it.** Changing the active show asks for confirmation (P8c) and is announced on
  every screen.

### 1.3 Presence

- **Screens list.** The status bar's **Screens** menu lists connected screens: operator, role, and
  last heartbeat.
- **Expected screens.** In show mode, an expected screen (wall, FOH, each assigned A2) missing
  for more than 30 s raises a NOTE ("A2 SR phone offline 0:45").
- **Uncovered zone.** A zone with no connected A2 raises a NOTE ("Nobody covering SR").

---

## 2. Vocabulary

| Concept | UI word |
|---|---|
| Handheld / bodypack transmitter | **mic** (Red, Bpack2) |
| IEM bodypack | **pack** (RX10) |
| IEM feed | **mix** (ME 3, Console mix) |
| IEM transmitter | **TX** (TX1). Packs on the same TX are "on TX1 with RX3, RX6" |
| Guitar wireless | **guitar pack** (GLX 1) |
| Receiver channel | **channel** (Ch 34) |
| Change between songs | **handoff**: *give* (stock → person), *collect* (person → stock), *pass* (person → person), *capsule swap* |
| Temporary pass with automatic return | **pass … and back** |
| Failed device replaced | **swap** / **on spare** |
| Device unusable | **out of service** |
| Device not returned | **missing** |
| Device brought by someone | **guest device** (owner shown) |
| Alert states | **NEW → ON IT → FIXED** (plus **cleared**: auto-resolved) |
| Battery evidence | **not checked**, **✓ OK**, **✓ fresh**, **LOW**, **manual**, live bars/minutes |

"Kit", "carrier" and "base" never appear outside Admin → Devices.

---

## 3. Data concepts this spec relies on

These are deltas from the current schema. Full list in §19.

- **Device usage.** Per device per song: holder (performer), or "not in use", or "spare".
- **Pass.**
  - Fields: device, from, to, song, **during** or **before**, optional "and back after".
  - States: planned → done → returned.
  - Holder during the song = *from* until the pass is marked done, then *to*.
- **Handoff identity.** Every handoff has a stable ID derived from (device, from song ID, to song
  ID or pass ID). It survives reorders and is re-validated after edits.
- **Handoff states.** pending → done; **overdue** if the song moved on without it; **cancelled** if
  the plan changed.
- **Alert.** Fields:
  - device;
  - holder at raise, song at raise;
  - type (from the §7.6 table), priority, failed component;
  - repeat count, snoozed until song;
  - owner (resolved per §7.3).
- **Swap.** States: recorded → in hand → FOH confirmed. Records the failed component.
- **Device.** Adds: owner (house / guest name), status (… missing, on loan), battery kind,
  telemetry capability, board section and order, TBC fields.
- **Show.** Adds: day, set (afternoon / evening), concert time, call time, **active** flag, linked
  shows (Friday/Saturday, afternoon/evening).

---

## 4. Shared chrome (every screen)

### 4.1 Status bar (always visible; 56 px, 72 px on the wall)

`[Event · Show ▾]  [● LIVE 0:02 | STALE 0:31 | OFFLINE]  [SHOW MODE]  [MIDI ✓ / 🔒 / ✕]  [⬣ 1] [▲ 3] [i 2]  [Screens]  [Operator ▾]`

- **Master counts.** Always present. Tap to filter alerts.
  - ⬣ critical: filled red with a white octagon and number.
  - ▲ warning: amber outline with a triangle.
  - i note: blue circle.
- **Connection.**
  - **LIVE** shows seconds since the last server message.
  - **STALE** (> 25 s): yellow-black hatched bar.
  - **OFFLINE**: full-width banner (§11).
- **Show name.** Truncates from the left; the day and set always stay visible.

### 4.2 Song strip

`[◀]  NOW 2/3 · I Will Follow Jesus / Lace Up Your Boots  [▼ setlist]   NEXT: Concert (after)   [▶]`

- **Wrapping.** The title wraps to two lines on phones; it is never cut.
- **Setlist drop-down.**
  - Done songs greyed.
  - Skipped songs struck through.
  - "Jump to…" on any song; a jump of more than one asks for confirmation.
- **Control rights.**
  - Lead and FOH: tap ◀ ▶.
  - A2 phone: **press and hold 0.6 s** (fill animation).
  - Helper / Viewer: hidden.
- **MIDI changes.** A song change from MIDI shows "via Playback" for 5 s. A **jump** from MIDI
  (skip) also raises a NOTE "Playback jumped from Song 2 to Song 4" listing the combined handoffs
  (§9.4).

### 4.3 Activity line (replaces toasts)

- **One line under the song strip.** Shows the latest event: "Sam: Red ✓ OK · 4 s · Undo".
- **Undo.** Available for 10 s when allowed (P8, reliability req 21).
- **Activity log.** Tapping the line opens the last 50 events.
- **Never overlaps content.** Criticals never use it.

### 4.4 Alert column / strip

- **Placement.**
  - Wall and FOH: a right column at least 420 px wide.
  - Phone: pinned under the song strip, collapsing to one line.
- **Critical takeover.** A NEW critical also shows as a **96 px band across the top of the wall**:
  "⬣ RED · NO AUDIO · Helen · SL", readable at 3 m. It clears to the column when someone taps
  **On it**.

---

## 5. Device tile

### 5.1 Two axes

Every tile shows **usage** (one value) plus a **health overlay** (the highest value wins).

**Usage:**
- **In use**: assigned in the current song.
- **Not this song**: assigned in a later song. Line: "Next: Song 3 → Judah".
- **Done**: not used again this show.
- **Spare**: designated spare, ready.
- **On spare**: a spare currently replacing a failed device. Line: "for Red".
- **Guest**: brought by a guest; owner shown.
- **Out**: out of service. Diagonal hatching; stays in place.
- **Missing**: count-in not returned.

**Health**, highest first:
- CRITICAL > WARNING > STALE DATA > MANUAL-DUE > OK.

**Visual rules:**
- **Usage changes:** background tint and an icon, never text opacity.
- **Health changes:** the border plus a status word and glyph on the right of the header.
- **CRITICAL:** a 6 px red border and a red header with a white "⬣ NO AUDIO". Pulses at 1 Hz
  until ON IT; with reduced motion it is a static double border.
- **WARNING:** a 3 px amber border and "▲ LOW BATT".
- **STALE DATA** (Phase 5): the telemetry row is hatched, "no data 0:45".

### 5.2 Layout

```
┌──────────────────────────────┐
│▌RED  Ch 34            ▲ LOW  │  header: chip stripe + ID (bold) + channel/mix | status word
│ (photo) Helen                │  holder: first name (+ initial if ambiguous)
│ ✓ 18:42 Sam   or ▮▮▮▯▯ 2:10   │  battery evidence / telemetry
│ Next: → Shosh (pass, Song 2) │  only when the next song changes this device
└──────────────────────────────┘
```

- **IEM header:** "RX10 · ME 3".
- **Guitar header:** "GLX 1 · Ch —".
- **TBC fields** show a dotted underline and "TBC" (for example "Seth · mix TBC").

### 5.3 Tap behaviour

- **Tap:** opens the device panel (§6).
- **Long-press:** a quick menu; every item is also in the panel.

---

## 6. Device panel

The panel is a side panel on the wall and iPad, keeping the alert column visible. On phones it is
a bottom sheet.

### 6.1 Header

- Chip, ID, channel/mix, holder photo and full name, zone.
- For IEM packs: "on TX10 · mix ME 3 · also on TX10: —".
- "Next: …"

### 6.2 Primary actions (64 px)

- **✓ Batt OK** (primary)
- **Fresh batts**
- **LOW**
- **Problem…**
- **Swap…** (hidden for Helpers)

### 6.3 Problem presets

These come from the §7.6 table:
- No audio
- Crackle
- Dropout / RF hit
- Low battery
- Wrong pack / mic
- Sweat
- Cable / connector
- Other (optional text)

After choosing a preset, a second row asks **"What failed?"** with these options:
- Mic/pack
- Capsule/headset
- Channel/receiver
- Don't know (default)

This answer is optional and never blocks.

### 6.4 Other sections

- **This show:** a mini plot row with the holder per song; the current song highlighted.
- **History:** checks, problems, swaps, passes, with who and when.
- **Lead-only:** Out of service / Back in service (reason chips); Mark missing / Found; Edit device.

**Acceptance:**
- Batt OK = tap tile + tap ✓ (2 taps).
- Every action appears in the activity line within 1 s on every screen.

---

## 7. Alerts

### 7.1 Reporting

- **From a tile or FOH row:** Problem → preset = 2 taps. Device, holder, song and time are taken
  from context.
- **From the Problem button** (phone tab bar, FOH header):
  - First, a device picker: a grid of chips in board order, with the holder name under each.
  - Then the preset.

### 7.2 Lifecycle

- **NEW:** pulses; master count; critical takeover band on the wall.
- **ON IT:** someone taps **On it**. Shows "Sam is on it · 0:12" and stops pulsing.
- **FIXED:** someone taps **Fixed**. The note is optional and can be added later in the log.
- **cleared:** a rule or telemetry condition ended. Shown greyed for 30 s, then logged.
- **Snooze** (WARNING only): **until next song**. It returns automatically and shows a "snoozed"
  count. CRITICAL can't be snoozed.
- **Duplicates:** the same type on the same device within 2 min adds "+1 FOH" to the existing
  alert.

### 7.3 Ownership: who is expected to act

The owner is the first of:
1. the A2 owner of the current holder;
2. the A2 covering the device's zone;
3. everyone.

**Ownership effects:**
- The owner's phone shows the alert at the top of My next.
- On the wall, the owner's name sits in the row ("for Sam").
- If nobody taps ON IT within 30 s, the owner widens to everyone, and the row shows "unowned".

### 7.4 Display rules

- **Order:** the column sorts critical, then warning, then note; newest first within each.
- **Limit:** at most 5 individual rows, then "+N more".
- **Grouping:** rule alerts of one type collapse into one row ("4 packs: battery not checked
  →"). Critical alerts are never grouped.
- **Suppression:** devices in usage *Done* never raise WARNING or CRITICAL from rules.
- **Shared-TX prompt:** a problem on a pack whose TX feeds other packs prompts:

  > "RX3 and RX6 are also on TX1. Same problem there?" [Yes, likely TX1] [No, just RX1]

  "Yes" re-points the alert to TX1 and flags all three packs.

### 7.5 Sound and vibration

- **Wall and FOH:** optional single tone for NEW critical only; off by default.
- **Phones:**
  - D5 = HTTP: phones don't alarm; the wall, FOH and comms carry alarms.
  - D5 = HTTPS: the installed app gets notifications for criticals the phone owns.
- **iOS:** `navigator.vibrate` isn't available on iOS and is used only where supported.

### 7.6 Alert type table (the single source of priorities)

| Type | Raised by | Priority | Expected response |
|---|---|---|---|
| No audio | Person | CRITICAL if in use now, else WARNING | Check / swap now |
| Dropout / RF hit | Person; Phase 5 rule | CRITICAL if in use now, else WARNING | Swap at next gap |
| Crackle | Person | WARNING | Swap at next gap; capsule/cable likely |
| Low battery | Person; rule (Phase 5 bars ≤ 1 or < 30 min) | WARNING; CRITICAL if in use now and ≤ 15 min | Fresh batts at next gap |
| Wrong pack / mic | Person | WARNING | Correct at next gap |
| Sweat / cable / other | Person | WARNING | At next gap |
| Battery not checked | Rule, show mode, device used later | WARNING (grouped) | Check before its song |
| Handoff overdue | Rule (song passed, not done) | WARNING | Do it or cancel it |
| Swap not confirmed by FOH | Rule (> 30 s) | WARNING to FOH | Confirm at console |
| MIDI cue with no song | Rule | WARNING | Fix the cue or ignore |
| MIDI bridge offline | Rule, show mode | WARNING | Advance by hand |
| Playback jumped songs | Rule | NOTE | Read combined handoffs |
| Screen offline / zone uncovered | Rule, show mode | NOTE | — |
| Interference / encryption mismatch / TX lost (Phase 5) | Telemetry | CRITICAL if in use now, else WARNING | Swap / check |

---

## 8. Battery

### 8.1 Evidence shown on every device

| Situation | Tile shows |
|---|---|
| Never checked | "not checked" (neutral grey). Becomes ▲ CHECK BATT from call time, or in show mode, if used later in the show |
| Checked today | "✓ OK 18:42 Sam" / "✓ fresh 18:40 Sam" |
| Checked another day | "✓ Fri 19:02" (amber after call time) |
| LOW | ▲ LOW (§7.6) |
| Telemetry (Phase 5, ULX-D) | ▮▮▮▯▯ and minutes on SB900; bars on AA; "≈" if older than 60 s |
| Can't report (XSW, GLX, IEM packs) | Manual evidence only, with a small "manual" tag |

- **Battery kind.** Each device has a battery kind (AA alkaline / AA NiMH / AA lithium / SB900 /
  SB902). Checks record it automatically; it can be changed in the panel.

### 8.2 Battery service screen (wall and phone)

- **Layout.** Devices in board order, filtered by **section** (Mics / IEM / Guitar) and **zone**
  (SL / SR / All).
- **Cells.** Each cell has **✓ OK** (tap) and **Fresh** / **LOW** buttons.
  - The first tap gives ✓ OK. Tapping again undoes it.
  - There is no cycling toggle.
- **Progress.** A counter reads "SL mics: 6/8 checked".
- **No "all" shortcut.** Each check is a deliberate tap; 28 taps take about 40 s.
- **Section done.** **"SL mics done"** is enabled only when every cell in the section has a state,
  and it records who closed the section.

### 8.3 Call time

Call time defaults to **concert time − 1 h** (per the Homestead sheet) and is editable per show.

---

## 9. Handoffs

### 9.1 Generated list

Handoffs are generated from the plot and passes:
- **give** (stock → person)
- **collect** (person → stock)
- **pass** (person → person)
- **pass … and back**
- **capsule swap**

### 9.2 Timing

- **"Before Song N":** due at the song boundary.
- **"During Song N":** a pass. Appears as a NOW card when Song N becomes current, and is done
  manually when the A2 taps **Pass done**. Marking it done changes the holder; the board, FOH and
  alerts all follow.
- **"After the show":** collects.

### 9.3 States

- **pending → done**, with who and when.
- **Overdue:** the song moved past its due point without the handoff being done. It gets a
  WARNING on the device; the options are **Done now** or **Cancel**.
- **Cancelled:** the plan changed (§13). It shows struck through for 30 s with the reason.

### 9.4 Jumps and going back

- **MIDI or manual jump from song A to song C.** The handoffs shown are the *combined* change from
  A to C. Handoffs belonging to the skipped song B that aren't needed by C are cancelled with the
  reason "skipped".
- **Going back (C → B).** RF Ops shows a "Reverse?" card listing what moves back. It never
  auto-undoes completed handoffs; the A2 confirms each one or ignores it.

### 9.5 Ticking

- **Give and collect** also record checkout and return. These feed the count-in (§12.3) and the
  "last holder".
- **Pass done** records the holder change.
- Each handoff card carries a **hygiene line** when the device changes person:
  - "Wipe handheld" for mics.
  - "Own earpieces / wipe pack" for IEM.

---

## 10. Swap to spare

### 10.1 Start

- **From an alert:** Swap (1 tap).
- **From a device panel:** Swap… (1 tap).

### 10.2 Swap panel

- **Header:** "Swap Helen off **Red** (Ch 34)".
- **Candidates**, best first. Each is a 72 px card with chip, channel, battery evidence, location
  and receiver unit.
  1. Designated **spares** of the same type: battery ✓ or untested, not in use.
  2. **Free devices** of the same type: not used in the current song. Labelled "free until
     Song 5", or "free rest of show".
  3. Same receiver unit as the failed one: labelled "same receiver" (acceptable, flagged).
- **No candidate:** "No spare or free Red-type mic. Options: [Capsule swap / fix in place]
  [Pull Red (mute at FOH)] [Tell FOH]". Each option logs and notifies FOH.

### 10.3 Options (defaults shown)

- **Scope:**
  - **Until Red is fixed** (default): the holder stays on the spare for later songs until Red is
    back in service and someone taps "Return to Red".
  - **This song only.**
- **What failed:** Don't know / Mic / Capsule / Channel. If you pick one, it is marked out of
  service.

### 10.4 Swap button

The button text is the full action: **"Swap Helen: Red → Bpack3 (Ch 43)"**.

### 10.5 Two-step physical flow

1. **Recorded**, when tapped:
   - The board shows "Bpack3 · ON SPARE for Red · getting ready".
   - FOH gets a **Swap card**:
     - "Helen moving **Ch 34 → Ch 43** (Bpack3)."
     - "ME-1: direct-out of Ch 43 needed on Helen's ME slot."
     - "Wait for **In hand** before unmuting."
2. **In hand:** the A2 taps **In hand** when the spare is on the performer. The FOH card turns
   amber: "In hand. Unmute Ch 43 / mute Ch 34". FOH can act on comms before this if needed.
3. **FOH confirmed:** FOH taps **Done at console**. Backstage sees "FOH ✓ 0:08". If not confirmed
   within 30 s of In hand, FOH gets a WARNING.

**IEM pack-only swaps:** the FOH card says "**Pack only — no console change**". FOH still taps
Done (D2), so they know.

### 10.6 Undo and return

- **Undo:** allowed until In hand. After In hand, use "Reverse swap", which creates a new FOH card
  ("Swap reversed: Ch 43 → Ch 34").
- **Return:** when Red is back in service, its panel offers **Return Helen to Red**, which runs the
  same two-step flow.

---

## 11. Offline and degraded operation

Reliability reqs 12–17.

- **STALE** (> 25 s without a server message):
  - hatched status bar;
  - telemetry greyed;
  - risky actions (swap, song control, plot edits) disabled with "Reconnecting…".
- **OFFLINE banner:** "OFFLINE since 20:14:05 — follow your paper sheet. Tell FOH on comms." The
  board desaturates.
- **Pending queue.** Low-risk taps (Batt OK / Fresh / LOW, handoff Done, Pass done, Problem)
  are **queued** on the device:
  - shown with a "⏳ not sent" badge;
  - sent automatically on reconnect, with the original time and song;
  - idempotent.
- **Reconnect summary.** "While you were offline: 3 changes, 1 alert", with a link to the log. A
  critical alert on *my* devices that is still NEW notifies once.
- **Song check.** If the bridge saw cues while the server was down (it logs them), or if Playback
  is ahead, ask: **"Playback is on Song 4, RF Ops is on Song 2 → Go to Song 4?"**
- **Paper fallback.** The pre-show checklist includes printing these from Show → Print (§15.3):
  - the plot;
  - each A2's handoff sheet;
  - the FOH channel sheet.

---

## 12. Checklists

These are DO-CONFIRM pause points with at most 9 items each. They are reached from the status bar
("Pre-show 6/9"). Every item can be ticked by any A2 or the Lead; some tick automatically.

### 12.1 Pre-show

The list is editable; defaults:
1. Battery service done for every device used today (auto, §8.2)
2. Line check: every mic talked into (per-device tick in Battery service, "Line ✓" column)
3. IEM packs on the right TX, volume set, limiter on
4. Spares batteried, tested and staged; location noted
5. A2 Wi-Fi 5 GHz channels 36–64 only; no 2.4 GHz SSID
6. dLive scene/patch checked; FOH confirms the mic order on the board
7. MIDI bridge online and the test cue received (auto)
8. No plot conflicts (auto)
9. Paper sheets printed

**Enter show mode** shows any open items. "Enter anyway" is allowed for the Lead only.

### 12.2 Changeover (between sets)

1. Collect everything from the outgoing set (auto from collects)
2. Wipe handhelds and packs; own earpieces
3. Battery service for the incoming set
4. ME-1 scene for the incoming set loaded (FOH)
5. Switch the active show to the incoming set (confirm)

### 12.3 Post-show / count-in

1. **Count-in:** every device ticked as returned. Any not ticked appear as **Missing** with:
   - the last holder (photo, zone);
   - the last song;
   - the last event time.

   Guest devices show "return to owner".
2. Batteries out / rechargeables on charge
3. Wipe down
4. Faults reviewed (out-of-service list + notes)
5. Report saved/printed

---

## 13. Plan changes during the show

- **Who can change what in show mode:**
  - Lead: setlist (add / skip / reorder) and plot cells.
  - A2s: skip only.
  - Every change is attributed.
- **Plan-diff card.** After any change, every A2 sees a **plan-diff card** with what changed for
  them, for example:
  - "RX10 pass moved: before Song 3 → during Song 5";
  - "+1 give: Chandler ← Blue".

  They acknowledge it with **Got it**.
- **Program Change numbers belong to the song, not its position.**
  - Reordering never renumbers.
  - "Number 1, 2, 3…" is available in setup only and warns: "Make sure Playback's cues match."
- **Last-minute guest device** (Lead or A2, allowed in show mode):
  - **Add guest device:** type, label, owner, and for IEM which TX/mix it listens to.
  - It is created for this show only and appears in the board's Guest section.
  - It can **replace** a planned device for a performer ("Chandler uses his own pack instead of
    RX9"). RX9 becomes Done for this show, so it raises no battery alert.

---

## 14. Screens

### 14.1 Wall board

**Hardware.** A 32″ 1080p display is recommended. The touch distance is arm's length; glance
distance is 1–3 m.

**Layout at 1920×1080:**

| Area | Size |
|---|---|
| Status bar | 72 px |
| Song strip | 80 px |
| Activity line | 36 px |
| Critical band (when present) | 96 px |
| Board | 1460 px wide |
| Alert + handoff column | 440 px |

**Board:**
- **Sections in fixed order:**
  - MICS (12: 2 rows of 6)
  - IEM (up to 13: 3 rows)
  - GUITAR (3)
  - SPARE / GUEST (as needed)
- **Grid:** 6 columns × 236 px; rows about 140 px (fits 6 rows in the 890 px available).
- **Overflow:** if there are more devices, the board switches to a **compact density** (holder
  name and status only, 110 px rows) rather than scrolling.

**Type** (32″ panel ≈ 0.37 mm/px):

| Element | Size |
|---|---|
| Device ID | 28 px bold |
| Holder name | 30 px |
| Status word | 24 px bold |
| Battery line | 20 px |
| Alert titles | 32 px |
| Critical band | 72 px |

- **Size setting:** S/M/L per screen in Settings, for smaller or farther displays.
- **Names:** first name only on tiles. "First L." when two holders share a first name. Device IDs
  and status words are never cut.

**Column:**
- Alerts (§7.4)
- **Next handoffs:** the current boundary, plus a collapsed preview of the following one.
  - Order: pass and capsule swap first, then give/collect grouped per person.
  - 64 px rows, each with a **Done** button.

**Filters:** All · Mine · SL · SR · IEM · Guitar.

**Theme:**
- Dark: bg #121418, text #C9D1D9.
- **Dim** button: overall brightness to 60%; alerts stay at full brightness.
- Light theme for load-in.

**Kiosk:** `?kiosk=1`:
- hides navigation;
- disables pinch-zoom (wall only) and selection;
- returns to the board after 60 s idle;
- has a corner menu (Settings, Dim, Battery service, Checklists, QR join).

### 14.2 Phone (A2 / Helper)

**Tab bar** at the bottom: **Next · Board · Alerts · Check · Problem**.
- Problem sits in the tab bar, not floating.

**Next tab** (default):
- **Pinned on top:** my NEW/ON IT alerts.
- **Then:** my handoff cards in time order. Each 120 px card shows:
  - the action ("PASS RX10 → Shosh");
  - photos;
  - when ("during *I Will Follow Jesus*");
  - where (zone);
  - the hygiene line;
  - **Done** (64 px) and **Problem**.
- **Plan-diff cards** appear here.

**Other tabs:**
- **Board:** tiles in a 2-column list with filter chips.
- **Alerts:** full list.
- **Check:** battery service and checklists.

**Song strip:** one line. Hold-to-advance for A2; hidden for Helpers.

**Sizes:** primary targets ≥ 48 px; Done/Problem 64 px; body text ≥ 17 px. Zoom is never
disabled on phones.

### 14.3 FOH: Channels

- **Rows (48 px):** Ch · chip + device · NOW · NEXT · battery evidence · status.
- **Order:** mics by console channel, then IEM by RX number, then guitar.
- **NEXT column** shows the change: "→ Shosh", "off", "↩ back", "pass during".
- **[!] per row:** 2-tap Problem.
- **Right column:**
  - **Swap cards** at the top (§10.5), each with **Done at console**;
  - then alerts;
  - then messages.
- **FOH dim:** a low-glare variant in which criticals use a dark-red fill rather than full
  saturation, to keep light off the audience.
- **Quiet:** no sound and no motion except a NEW critical.

### 14.4 Report (§16)

---

## 15. Admin

### 15.1 Plot (mic plot)

- **Rows:** devices in board order. **Columns:** Default + each song; done songs are greyed.
- **Cells:** the holder name.
  - Tap → people picker (search, recent, "— none", "↳ default").
  - "↳" means the default applies.
- **Visible buttons:** **Fill right**, **Copy column**, **Clear column**. Drag-to-fill is a
  shortcut only.
- **Pass:** **Pass in this song…** on a cell creates a during-song pass (to whom, and back after?).
- **Conflicts:** red cells with the reason in the cell tooltip and in a list under the grid.
- **Person view:** a toggle (person rows), read-only.
- **Show mode:** locked except for the Lead (§13).

### 15.2 Devices

- **One editable table:** ID, type, section, board order, channel/receiver, capsule/element,
  console ch, TX/mix, battery kind, spare flag, owner, status, notes, TBC flags, telemetry
  capability (read-only).
- **Bulk add:** "8 handhelds named …".
- **CSV:** import/export kept.
- **Receivers sub-table** (Phase 5): IP, model, firmware, connection state, meter rate.

### 15.3 Show

- **Fields:** name, **day**, **set**, concert time, call time (default −1 h), mode, linked shows.
- **Setlist:**
  - paste box (one per line);
  - drag reorder;
  - skip;
  - PC number per song (bound to the song).
- **Print:** plot, per-A2 handoff sheets, FOH channel sheet (print CSS, A4/Letter).
- **Shows across days and sets:**
  - **Compare** two shows side by side (diff of plot cells).
  - **Apply to…** copies selected rows or changes to linked shows.
  - **Duplicate** a show as a template.

### 15.4 People

- **Fields:** name, role, **zone** (managed list: SL / SR / Center / Riser / Pit / FOH), A2 owner,
  photo, notes, **not here tonight** (per show; cancels their handoffs with the reason "absent").
- **Photo:** **Take photo** uses `<input type=file accept=image/* capture>`, which works over
  HTTP. Images are resized to 256 px on the server.
- **Bulk:** select many → set zone / A2 owner.

### 15.5 Operators

- Add / remove; PINs.
- **Add helper** (§1.1).
- Practice show.

### 15.6 MIDI, Data

As built, plus the health page link (reliability req 26).

---

## 16. Report

Report sections, in this order:
1. **Open problems:** out-of-service devices, missing devices, alerts not fixed (the "pull for
   repair" list).
2. Swaps, with the failed component and FOH confirm time.
3. Alerts, with time to ON IT and time to FIXED.
4. Overdue or cancelled handoffs.
5. Battery checks.
6. Song changes (source).
7. Alert counts per 10 minutes, plus the "noisiest alert types" (bad-actor review).

---

## 17. Quick messages (D3)

- **Phrases** (editable list):
  - "Check [device]"
  - "[device] is hitting"
  - "Go to spare on [device]"
  - "Pull [device] after this song"
  - "Copy"
  - "Standing by"
  - "Thanks"
- **Composing.** A device chip is attached by tapping a tile or row first, or by choosing from the
  picker.
- **Recipients:** FOH, a specific A2, or All.
- **Urgent:** a separate **Urgent** button flashes the recipient's alert column once and asks for
  acknowledgement (read receipt).
- **No free text** in show mode except for the Lead.

---

## 18. Visual system

- **Neutrals:** bg #121418 · panel #1A1E24 · line #2A3038 · text #C9D1D9 · muted #8B949E.
  All text meets 4.5:1; primary wall text and alert titles meet 7:1.
- **Status:**
  - CRITICAL #E5484D, filled, octagon glyph.
  - WARNING #F5A524, outline, triangle.
  - NOTE #7AA2F7, circle-i.
  - Status hues differ by ≥ 3:1 luminance.
- **Device chips:** an 8 px left stripe in the device colour plus the word. Black and White chips
  have a 1 px outline. Status is never shown as a stripe, and chips are never shown as a border.
- **Patterns:** hatching = out / missing / stale; dashed border = spare; usage = background tint.
- **Type:** system UI font; tabular numerals for times, channels and minutes.
- **Motion:** only NEW critical (1 Hz). Respects `prefers-reduced-motion`.
- **Colour-blind check:** every surface is checked in greyscale and in deuteranopia and protanopia
  simulation. Status remains identifiable by word and glyph.

---

## 19. Data model and API deltas

**Tables / fields:**
- **performers:** `photo_path`, `zone_id`, `absent_show_ids`; **zones** table.
- **equipment:**
  - `owner`, `section`, `board_order`, `battery_kind`, `telemetry_capability`, `tbc_fields`;
  - status adds `missing`, `on_loan`;
  - `show_id` for guest devices.
- **shows:** `day`, `set_name`, `concert_time`, `active` (one row), `event_id` (groups linked
  shows).
- **passes:** id, show, device, from, to, song, timing (before/during), `and_back`, state, done
  by/at, returned by/at.
- **handoff_state:** stable `handoff_key`, state (pending/done/overdue/cancelled), reason,
  by/at. Replaces `handoff_done`.
- **alerts:**
  - `type`, `holder_at_raise`, `song_at_raise`, `failed_component`;
  - `repeat_count`, `snoozed_until_song_id`, `owner_operator_id`, `on_it_by/at`.
  - The device is required for device alerts.
- **swaps:** `state` (recorded/in_hand/foh_confirmed/reversed), `in_hand_at/by`,
  `foh_confirmed_at/by`, `failed_component`, `scope` (until_fixed/song).
- **battery_checks:** `battery_kind`, `line_check` (bool), `section_closed_by`.
- **checklists / checklist_items / checklist_ticks.**
- **messages:** phrase, device, from, to, urgent, `acked_at/by`.
- **client_sessions:** presence. **join_codes.** **epoch** (standby).
- **Optimistic concurrency:** `version` on equipment, kits, assignments, songs and alerts.

**Endpoints** (new):
- Active show
- Passes CRUD + done/return
- Handoff state
- Swap in-hand / foh-confirm / reverse / return
- Alerts on-it / snooze / repoint-to-TX
- Guest device
- Count-in / missing / found
- Checklists
- Messages + ack
- Photo upload
- Join code
- Presence
- Plan diff (event)
- Print views
- Compare / apply-to shows
- Health
- Practice show

**Events** (new): `tick` (heartbeat, server_time, epoch). Clients never notify on events older
than their last seen sequence, except still-NEW criticals they own (once).

---

## 20. Acceptance tests

Automated in Rust and Playwright unless marked (manual).

1. **Speed.**
   - Raise → visible everywhere < 1 s.
   - Swap → FOH card < 1 s.
   - Batt OK 2 taps; swap 3 taps from an alert; report 2 taps from a FOH row.
2. **Stable layout.** Tile boxes are identical across every song change and every alert state.
3. **No status colour when healthy.** A pixel scan of the healthy state, excluding chip stripes,
   finds no status red or amber.
4. **Colour-blind.** Every state is identifiable in greyscale and in simulated deuteranopia /
   protanopia (word + glyph present). Manual review checklist plus automated DOM checks.
5. **Replay.** A fresh load never toasts, vibrates or pulses for old events. A still-NEW owned
   critical notifies once.
6. **Grouping.** 28 rule alerts produce ≤ 5 rows plus a group row.
7. **Passes.** RX10 pass during *I Will Follow Jesus*:
   - before Pass done, the holder is Helen and the alert follows RX10;
   - after Pass done, it is Shosh;
   - the return happens at the boundary.
8. **Overdue handoffs.** MIDI jump 2 → 4: handoffs show the combined change; skipped-only
   handoffs are cancelled with "skipped"; an un-ticked pass becomes overdue.
9. **Offline queue.** Phone offline → Batt OK and Done are queued with a badge → reconnect → applied
   once, with the original song.
10. **Swap flow.** Recorded → In hand → FOH Done; the IEM pack-only wording; undo before In hand;
    reverse after it.
11. **Plan diff.** A lead reorder in show mode produces per-A2 diff cards; PC numbers unchanged.
12. **Guest device.** Add Chandler's pack replacing RX9: RX9 is Done with no battery alert.
13. **Count-in.** A missing device shows the last holder and song.
14. **Touch.** Wall targets ≥ 64 px; phone primary ≥ 48 px; Done/Problem ≥ 64 px.
15. **Contrast.** Text ≥ 4.5:1; wall primary and alert ≥ 7:1 (automated contrast audit).
16. **Reliability.** All requirements in `docs/reliability.md` §5.
17. **Volunteer test (manual).** A first-time helper, with the 3-card intro only, completes:
    battery service for their section, a give, a pass, and a Problem report, without help.
18. **Crew review (manual).** Two A2s and the FOH engineer run the practice show before the first
    real use.

---

## 21. v1 → v2 change log (independent review findings)

| Review item | Resolution |
|---|---|
| Tile "one state" impossible | §5.1 usage × health axes with precedence |
| Grid doesn't fit; no guitar; long names | §14.1 recalculated; GUITAR + SPARE/GUEST sections; name rule; compact density |
| Swap flow breaks with no spares; Ch 42 example wrong | §10.2 candidates incl. free devices + no-candidate options; example uses real channels |
| Recording vs physical swap conflated; ME direct out; pack-only | §10.5 recorded → in hand → FOH confirmed; ME-1 line; "pack only" |
| Mid-song RX10 pass | §3 passes with "during" timing; §9.2 |
| MIDI skips drop handoffs | §9.3 overdue, §9.4 combined jump, "skipped" cancellation |
| Setlist edits / PC renumbering / stale ticks | §13 policy, plan-diff, PC bound to song, stable handoff keys |
| "All fresh" fabricates evidence | §8.2 removed; per-device taps; section close |
| Offline / reboot handling | §11 + `docs/reliability.md` |
| iOS limits | §7.5, D5 decision |
| Days and sets | §1.2 active show, §12.2 changeover, §15.3 compare / apply-to |
| Guest devices, missing, owner | §13, §12.3, §19 |
| Contradictions (LOW severity, presets, confirmations, "nothing before call", chip vs scan) | §7.6 single table; §6.3 presets; P8 rule; §8.1 evidence; P3 + §20.3 chip exclusion |
| Alert ownership | §7.3 |
| Multi-user undo | §4.3 + reliability req 21; §10.6 |
| Failed component; shared-TX cascade | §6.3, §10.3; §7.4 shared-TX prompt |
| Pocket advances | §4.2 hold-to-advance; helper hidden |
| Presence and onboarding | §1.1, §1.3 |
| Flood mode / 15-min snooze / long-press-only / cycle toggles | Removed / "until next song" only / P7 / §8.2 |
| Dim by opacity; chip outlines | P2, §18 |
| Vocabulary "pass" | §2 |
| Hygiene | §9.5, §12.2 |
| TBC visual | §5.2 |
| Volunteer card | §1.1 3-card intro + §20.17 |
| Message read receipts | §17 |
| Not here tonight | §15.4 |
| Hardware: TX_DEVICE_ID not on ULXD1/2 | Wrong-pack check is manual (preset); `docs/hardware-integration.md` §1 |
| Hardware: Playback fires on play only | §4.2, hardware §5 (cue at 0:00 of each song) |

---

## 22. Build order

1. **Reliability foundation:** DB pragmas, atomic backups, LaunchDaemon, heartbeat/STALE, resume,
   server time, health page, presence.
2. **Data model:** passes, handoff state, alert fields, swaps states, devices/people/shows fields,
   active show, plus migrations and seeds (HF template updated with sections and battery kinds).
3. **Shared UI:** status bar, song strip (hold-to-advance), activity line + undo, alert
   column/strip + critical band, device tile, device panel, problem presets.
4. **Wall board** + next handoffs + battery service.
5. **Swap** (two-step) + FOH channels + swap cards.
6. **Phone tabs** + offline queue + plan-diff cards + onboarding (join code, intro).
7. **Admin:** plot (passes, fill right), devices table, show (day/set, print, compare/apply),
   people (photos, zones, absent), operators (helpers, practice).
8. **Checklists**, count-in / missing, quick messages, report.
9. **Full acceptance suite** (§20) + reliability tests + soak + screenshots for crew review.

Then Phase 5 telemetry attaches to the tile's battery/RF row and the §7.6 rules, with no layout
changes.
