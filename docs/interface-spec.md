# RF Ops interface spec — v3.1

**Status:** v3.1 (Nov 2026). Supersedes v2.
- v3 resolved the second independent review's 17 blockers (§23).
- v3.1 resolves the third review's 9 blockers (§24).

**Builds on:**
- the build audit (`docs/ux-audit.md`);
- four research tracks: A2 practice, comparable products, alarm/HMI standards, reliability;
- primary-source hardware facts (`docs/hardware-integration.md`);
- an independent review of v1. Its 26 findings are each resolved in §21;
- an independent review of v2. Its 17 blockers and 25 other findings are resolved in §23.

**Companion documents:**
- `docs/data-model.md`: **authoritative** tables, derived values (holder, handoffs, usage) and
  migration.
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
| **Helper** (volunteer A2) | Phone | My next | Handoff Done, Pass done, battery and line checks, report Problem, On it. No swaps, song control, Fixed on criticals, plot edits or admin |
| **Viewer** | Any | Board | Read only |

### 1.1 Signing in and onboarding

Security detail is in `docs/hosting-and-security.md` §Security 2–6.

- **Leads, A2s, FOH** sign in with name + **PIN** (mandatory; there is no name-only sign-in).
- **Helpers: join code.**
  - The Lead taps **Add helper**, types a name, picks a zone (SL/SR/…), and taps Add.
  - RF Ops shows a QR code plus a **6-digit code**. It works for **that helper on up to 3 devices
    for 2 h**.
  - Scanning signs the helper in, with their zone's performers already "mine".
  - **Manual entry:** the sign-in screen always has "Have a code? Type it". This is needed on
    iPhone: a Home-Screen icon has its own cookies, separate from Safari, so after adding the
    icon the helper types the same code once more.
  - **Re-issue all helper codes** (Admin → Operators) invalidates every outstanding code.
  - **On the wall:** the QR shows only for 60 s after a Lead enters their PIN.
- **Screens (wall, FOH).**
  - A Lead enrols each screen once: Admin → Screens → **Enrol this screen**, a name ("Wall SL"),
    and PIN.
  - The credential lasts 7 days (sliding), never expires during show mode, and can be revoked.
  - Wall actions are attributed "Wall"; tapping a name chip first makes them "Wall (Sam)" for
    5 min.
- **Assigning devices.** "Mine" for a phone = performers whose A2 owner is this operator, plus
  their zone. The Lead assigns zones in bulk (§15.4).
- **First open.** A first-time phone shows a **3-card intro**: "Your next jobs are here",
  "Tap Done when handed over", "Tap Problem if anything is wrong". Then it goes to My next.
- **Practice show.**
  - **Practice show** (from Admin) copies a show into a sandbox marked PRACTICE, with a diagonal
    "PRACTICE" watermark on every screen in it.
  - It has its own holdings, batteries, alerts and swaps, and cannot touch the real show or
    equipment status (`docs/data-model.md` §4).
  - It ignores MIDI unless "Practice with Playback" is on.

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
| Person-to-person change during a song | **pass** (the return at the next song boundary is an ordinary handoff) |
| Failed device replaced | **swap** / **on spare** |
| Device unusable | **out of service** |
| Device not returned | **missing** |
| Device brought by someone | **guest device** (owner shown) |
| Alert states | **NEW → ON IT → FIXED** (plus **cleared**: auto-resolved) |
| Battery evidence | **not checked**, **✓ OK**, **✓ fresh**, **LOW**, **manual**, live bars/minutes |

"Kit", "carrier" and "base" never appear outside Admin → Devices.

---

## 3. Data concepts this spec relies on

**`docs/data-model.md` is authoritative.** In brief:

- **Devices** (Red, Bpack2, RX10, GLX 1) are global and shared by every show.
- Each show has a **plot**: one row per device, one column per song plus **Default**, and a
  person (or nobody) in each cell. A cell can also name a different capsule or headset.
- **Holdings** record who physically has each device right now. They carry over between shows,
  so afternoon → evening and Friday → Saturday just work.
- **A handoff is any difference** between the planned holder for the target song and the
  current holder. Doing it updates the holding. Nothing else is stored, so reorders, jumps,
  going back and count-in never leave stale ticks.
- **Passes** are person-to-person changes *during* a song (RX10 Helen → Shosh in *I Will Follow
  Jesus*). The return at the next boundary is an ordinary handoff.
- **Substitutions** cover:
  - swaps: this song, until fixed, or **rest of show**;
  - pulls (no replacement);
  - guest replacements.

  They overlay the plot without editing it.
- **Alerts** belong to a device (or a TX/receiver) and record the holder(s) and song at raise.
- **Song visits:** every song change is a visit. Going back to a song replays its passes.
- **Unusable devices** (suspect, out, missing) are never handed to anyone. A later planned use
  raises a conflict one song ahead instead.
- **Mix belongs to the IEM TX.** A pack moved to another TX hears that TX's mix.
- **Battery evidence** counts for a show only if recorded at or after that show's call time
  − 2 h.
- **Events** carry an IANA time zone. Every time is shown in it.

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

**Usage** (computed per `docs/data-model.md` §3.9; precedence out/missing > pulled > on spare >
in use > collect > next > spare > done):
- **In use:** planned in the current song.
- **Next:** not used now, used later. Line: "Next: Song 3 → Judah".
- **Collect:** not used again, but someone still holds it. Line: "Collect from Shosh".
- **Done:** not used again and back in stock.
- **Spare:** designated spare, ready.
- **On spare:** replacing a failed device. Line: "for Red".
- **Pulled:** failed with no replacement; the performer is off that channel. Line: "Pulled —
  mute Ch 34". Plain background with a slashed-mic icon.
- **Guest:** brought by a guest; owner shown.
- **Suspect / Out:** reported faulty. Suspect has a dashed border; Out has diagonal hatching and
  stays in place.
- **Missing:** count-in closed without it.

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

  "Yes" re-points the alert to TX1 and flags all three packs. A TX alert goes to the zones of
  everyone holding a pack on that TX. Its fix is a **TX swap** (Lead or FOH): "Move RX1, RX3 and
  RX6 to spare TX9 (retune packs)". The FOH card shows the ME change.

### 7.5 Sound and vibration

- **Wall and FOH:** optional single tone for NEW critical only; off by default.
- **Phones (Web Push, VAPID).**
  - After sign-in, the installed Home-Screen app asks once: "Get alerts for your devices?"
  - Pushes are sent only for NEW CRITICAL alerts the phone's operator owns, plus swap cards for
    FOH. Never for replayed history.
  - Needs iOS 16.4+ with the app on the Home Screen; Android Chrome works in the browser too.
  - Phones without push still show alerts when looked at. The wall, FOH and comms remain the
    primary alarm path.
- **iOS:** `navigator.vibrate` isn't available on iOS and is used only where supported.
- **Keep awake:** wall and FOH screens hold a Wake Lock, re-acquired on visibility change.

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
| Handoff overdue | Rule (difference against the current song) | WARNING | Done now, or Leave it |
| Replacement needed later | Rule, one song before the clash | WARNING | Find another / reassign / tell FOH |
| Swap: FOH confirm pending (FOH unreachable) | Rule | WARNING to the A2 | Tell FOH on comms |
| Swap not confirmed by FOH | Rule (> 30 s) | WARNING to FOH | Confirm at console |
| MIDI cue with no song | Rule | WARNING | Fix the cue or ignore |
| MIDI bridge offline | Rule, show mode | WARNING | Advance by hand |
| Playback jumped songs | Rule | NOTE | Read combined handoffs |
| Screen offline / zone uncovered | Rule, show mode | NOTE | — |
| Venue screens offline (phones still connected) | Rule | NOTE (one, combined) | Comms; FOH confirms later |
| Song check (stale buffered cue) | Rule | WARNING to the Lead | Go / Stay |
| Backup or R2 upload failing; disk < 25 % | Rule | WARNING to the Lead | Check /health |
| Disk < 10 %; no R2 upload for 10 min in show mode | Rule | CRITICAL to the Lead | Act before data is at risk |
| Login failures from the venue IP > 100 / 5 min | Rule | WARNING to the Lead | Check for abuse |
| Interference / encryption mismatch / TX lost (Phase 5) | Telemetry | CRITICAL if in use now, else WARNING | Swap / check |

---

## 8. Battery

### 8.1 Evidence shown on every device

**Evidence rule.** A check counts for show S only if recorded at or after S's **call time − 2 h**.
Times are in the event's time zone.

| Situation | Tile shows |
|---|---|
| No valid check for this show | "not checked" (neutral grey). Becomes ▲ CHECK BATT from call time if the device is used later in the show |
| Valid check | "✓ OK 18:42 Sam" / "✓ fresh 18:40 Sam" |
| Check from an earlier show today | "checked for earlier show (13:02)": amber from call time, and not evidence |
| Check from another day | "✓ Fri 19:02": amber from call time, and not evidence |
| LOW | ▲ LOW (§7.6) |
| Telemetry (Phase 5, ULX-D) | ▮▮▮▯▯ and minutes on SB900; bars on AA; "≈" if older than 60 s |
| Can't report (XSW, GLX, IEM packs) | Manual evidence only, with a small "manual" tag |

- **Battery kind.** Each device has a battery kind (AA alkaline / AA NiMH / AA lithium / SB900 /
  SB902). Checks record it automatically; it can be changed in the panel.
- **Practice shows** never create evidence for a real show.

### 8.2 Battery service screen (wall and phone)

- **Layout.** Devices in board order, filtered by **section** (Mics / IEM / Guitar) and **zone**
  (SL / SR / All).
- **Device zone.** The device's zone override, else the zone of its first planned holder in this
  show.
- **Cells.** Each cell has **✓ OK**, **Fresh** and **LOW**, plus a **Line ✓** column for mics
  (talked into).
  - Taps record a check immediately.
  - **Undo** is offered for 10 s, and only to the operator who tapped. Someone else tapping the
    same device just records a newer check.
  - There is no cycling toggle.
- **Several people at once.** Two A2s can work the same section in parallel. Each sees the
  other's ticks appear live, and the counter counts devices, not taps.
- **Progress.** A counter reads "SL mics: 6/8 checked".
- **No "all" shortcut.** Each check is a deliberate tap; 28 taps take about 40 s.
- **Section done.** **"SL mics done"** is enabled only when every cell in the section has a state,
  and it records who closed the section.

### 8.3 Call time

Call time defaults to **concert time − 1 h** (per the Homestead sheet) and is editable per show.

---

## 9. Handoffs

### 9.1 What a handoff is

A handoff is any difference between **who should have a device** (the plot, passes and
substitutions for the target song) and **who has it now** (the holding). `docs/data-model.md`
§3.4–3.8 has the exact rules.

**Kinds and button labels:**

| Kind | Label | Example |
|---|---|---|
| give | **Give Red to Helen** | Stock → Helen |
| collect | **Collect Red from Helen** | Helen → stock |
| pass | **Pass Red: Helen → Shosh** | Person → person |
| capsule swap | **Fit KSM9 on Red** | Same holder, different element |

The single action button is **Done** (64 px). It writes the new holding, attributed and undoable
for 10 s. The card's hint line names the physical act: "Hand it over, then tap Done".

### 9.2 Targets and timing

- **Now:** differences against the **current song**. Normally empty. Anything here is
  **overdue**: a WARNING on the device, with **Done now** and **Leave it** (a recorded exception
  for this song, cleared at the next boundary).
- **Coming up:** differences against the **next song**. This is "My next".
- **During this song:** passes in the current song, as NOW cards with **Pass done**.
- **Prep:** passes in the next song show one song ahead as "Get ready: RX10 Helen → Shosh during
  *I Will Follow Jesus*".
- **After the show:** collects, shown once the last song is current.

### 9.3 Who sees which card

- **Routing.** A card goes to the A2 who owns the **giving** side (holder now, or stock = the
  device's zone).
- **Passes across zones.** The receiver's A2 gets an info line ("RX10 coming to Shosh from SL").
- **Wall:** shows all of them.
- **Hygiene line,** when a device changes person: "Wipe handheld" for mics; "Own earpieces /
  wipe pack" for IEM.

### 9.4 Jumps, going back, reorders

- **Jump (MIDI or manual, 2 → 4):**
  - The new Now list is exactly the combined change; song 3's handoffs never appear.
  - Passes in jumped-over songs are cancelled ("jumped").
  - A pass in the departed song that wasn't done stays overdue only if its device still differs
    from the new plan. Otherwise it is dropped.
  - A jump never marks songs skipped, and needs no confirmation when it comes from MIDI.
- **Going back (4 → 3):** the Now list shows what moves back. Nothing is undone automatically; each
  card is done or left.
- **Reorder** (Lead, show mode): only songs after the current one can move. Handoffs recompute.
  Every A2 gets a plan-diff card (§13).

### 9.5 Count-in and last holder

- Holdings are the count-in (§12.3): anything not in stock at the end is still with its holder.
- **Last holder** = the holding, plus its `since` and the song it changed in.

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
  2. **Free devices** of the same type: not planned in the current song.
     - Labelled "free until Song 5" (amber: "needed by Judah at Song 5") or "free rest of show".
     - Choosing one that is needed later is allowed. One song before it is needed, RF Ops raises a
       conflict WARNING: "Tan needed for Judah at Song 5; Red still out" [Find another]
       [Give Judah a different mic] [Tell FOH].
  3. **Same receiver unit** as the failed one: labelled "same receiver" (acceptable, flagged).
  - **IEM packs:** candidates on the **same TX** come first. A spare on another TX is offered as
    either:
    - "RX9 — **retune to TX1**", which keeps the mix (sync the pack to TX1 before handing it
      over); or
    - "RX9 on TX9 — hears *Console mix*", which says on the FOH card that the ME source
      changes.

    "Pack only — no console change" appears only when the TX is unchanged or retuned.
  4. **Other form factor** (handheld ↔ bodypack/headset): a third tier, flagged "different mic
     type — tell FOH".
- **Capsule fix.** If a spare capsule or headset of the right type is in stock, the panel shows
  "Capsule swap (2 spare capsules)". With none in stock it is hidden.
- **No candidate:** "No spare or free Red-type mic." The options are:
  - [Capsule swap] (if any spare);
  - [**Pull Red**];
  - [Tell FOH], hidden when the reporter *is* FOH.

  Each option is logged and notifies FOH.
- **Pull** means the performer carries on with no working device of that kind for the scope
  chosen. The tile shows **Pulled — mute Ch 34**, and FOH gets a card: "Mute Ch 34 (Red, Helen).
  No replacement." [Done at console].

### 10.3 Options (defaults shown)

- **Scope:**
  - **Until Red is fixed** (default): Helen stays on the spare for later songs until someone taps
    **Return to Red**.
  - **This song only.**
  - **Rest of show: replace Red with Bpack3.** Every later Red cell, whoever holds it, moves to
    Bpack3. Use it when Red is dead for the night.
- **What failed:** Don't know / Mic / Capsule / Channel.
  - Picking one marks that part **suspect**, not out of service.
  - **Out of service** is a separate Lead-only action in the device panel, with confirmation
    (P8d).

### 10.4 Swap button

The button text is the full action: **"Swap Helen: Red → Bpack3 (Ch 43)"**.

### 10.5 Two-step physical flow

1. **Recorded**, when tapped:
   - The board shows "Bpack3 · ON SPARE for Red · getting ready".
   - FOH gets a **Swap card**:
     - "Helen moving **Ch 34 → Ch 43** (Bpack3)."
     - "ME-1: direct-out of Ch 43 needed on Helen's ME slot."
     - "Wait for **In hand** before unmuting."
2. **In hand:** the A2 taps **In hand** when the spare is on the performer and the failed device
   is back in their hand.
   - **This step records the physical move** (Bpack3 → Helen, Red → stock).
   - Until then, neither device appears in the handoff lists; the swap card is the only
     instruction.
   - The FOH card turns amber: "In hand. Unmute Ch 43 / mute Ch 34". FOH can act on comms before
     this if needed.
   - **Pull:** the tile reads "Pulled — mute Ch 34" at once. Collecting Red waits until the song
     ends: "Collect Red from Helen" appears as a Coming-up card.
3. **FOH confirmed:** FOH taps **Done at console**. Backstage sees "FOH ✓ 0:08". If not confirmed
   within 30 s of In hand, FOH gets a WARNING.

**IEM pack-only swaps:** the FOH card says "**Pack only — no console change**". FOH still taps
Done (D2), so they know.

**dLive line.** Mic swap cards add: "If Ch 43 isn't set like Ch 34, copy processing Ch 34 →
Ch 43."

**FOH unreachable** (venue screens offline, `docs/reliability.md` §3.4):
- The swap goes straight to **FOH confirm pending**.
- The A2 sees "Tell FOH on comms: *Helen to Ch 43, mute 34*".
- FOH confirms from the pending cards when back.

### 10.6 Undo and return

- **Undo:** allowed until In hand. After In hand, use "Reverse swap", which creates a new FOH card
  ("Swap reversed: Ch 43 → Ch 34").
- **Return:** when Red is back in service (**Mark fixed**), its panel offers **Return Helen to
  Red**.
  - It runs the same two steps; its In hand records Red → Helen and Bpack3 → stock.
  - Later planned holders of Red get it back as normal handoffs.
- **Unusable device planned later.** While Red is suspect, any later song that plans Red for
  someone not covered by the swap raises, one song ahead: "Red is suspect but planned for Judah
  at Song 6" [Extend the swap to Judah] [Pick a spare] [Mark Red fixed].

---

## 11. Offline and degraded operation

Reliability §3 and reqs 13–18.

**Two causes, two wordings:**
- **This device is offline:** "Your phone has no connection".
- **The server is unreachable:** "Can't reach RF Ops".

| State | Trigger | Behaviour |
|---|---|---|
| **STALE** | > 25 s with no server message | Hatched status bar; telemetry greyed. Risky actions (swap, song control, plot edits) are disabled with "Reconnecting…" |
| **OFFLINE** | > 60 s | Banner: "OFFLINE since 20:14:05 — Tell FOH on comms." The board desaturates |
| **Paper** | > 120 s | The banner adds "Follow your paper sheet" |

- **Offline shell.** A service worker caches the app shell only, never data, so a reload while
  offline still opens the app and shows OFFLINE instead of a browser error.
- **Pending queue (IndexedDB).** Low-risk taps (Batt OK / Fresh / LOW, Line ✓, handoff Done,
  Pass done, Problem) are queued on the device:
  - shown with a "⏳ not sent" badge;
  - sent automatically on reconnect, with the original time and song;
  - idempotent.
- **Acked log.** Commands acknowledged in the last 15 min are kept for replay after a new epoch
  (reliability §3.3).
- **Reconnect summary.** "While you were offline: 3 changes, 1 alert", with a link to the log. A
  critical alert on *my* devices that is still NEW notifies once. Replayed history never
  toasts, vibrates or pulses.
- **Venue screens offline** (phones on cellular): banner, swap "FOH confirm pending", and one
  NOTE (reliability §3.4).
- **Song check.** The agent posts its last buffered cue on reconnect. Under 10 s old, it is
  applied; older, the Lead is asked: **"Playback moved to Song 4 while offline → Go to
  Song 4?"**
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

1. **Collect** everything the incoming set doesn't use from the same person. Holdings carry over,
   so a performer in both sets shows **"Keep with Helen for evening"** instead of a collect.
2. Wipe handhelds and packs; own earpieces.
3. Battery service for the incoming set (new evidence window: call − 2 h).
4. ME-1 scene for the incoming set loaded (FOH).
5. Switch the active show to the incoming set (confirm).

### 12.3 Post-show / count-in

1. **Count-in:** the list shows every device whose holding is not stock: the holder (photo, zone),
   since when, and the song.
   - **Collect** (Done) returns it.
   - **Close count-in** (Lead) marks anything still out as **Missing**, keeping the last holder.
   - Guest devices show "return to owner"; Done means returned to the owner.
2. Batteries out / rechargeables on charge.
3. Wipe down.
4. Faults reviewed (suspect / out-of-service list, with notes).
5. Report saved/printed.

---

## 13. Plan changes during the show

- **Who can change what in show mode:**
  - **Lead:**
    - setlist: add, skip, and reorder **songs after the current one**;
    - plot cells for songs after the current one;
    - passes.
  - **A2s:** skip only.
  - Every change is attributed.
- **Plan-diff card.** After any change, each A2 sees what changed for their cards, recomputed
  from the handoff comparison, for example:
  - "RX10 pass moved: Song 3 → Song 5";
  - "+1 give: Chandler ← Blue".

  They acknowledge it with **Got it**. FOH gets diff cards for console-relevant changes (channel or
  person per channel).
- **Program Change numbers belong to the song, not its position.**
  - Reordering never renumbers.
  - "Number 1, 2, 3…" is available in setup only and warns: "Make sure Playback's cues match."
- **Last-minute guest device** (Lead or A2, allowed in show mode):
  - **Add guest device:** type, label, owner, and for IEM which TX/mix it listens to.
  - It exists for this show only and appears in the board's Guest section.
  - **Replace** (a guest substitution): "Chandler uses his own pack instead of RX9" moves
    only **Chandler's** remaining RX9 cells to the guest device. If nobody else uses RX9 later, it
    becomes Done, with no battery alert.
  - A guest device lasts the whole event (Friday and Saturday) and its label is unique within the
    event. **Return to owner** at count-in records that it went home.

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
- **Pass:** **Pass in this song…** on a cell creates a during-song pass (to whom). The return at the next song is automatic.
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

As built, plus the health page link (reliability req 27).

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

## 19. Data model and API

The tables, derived values, invariants and migration are in **`docs/data-model.md`**, which wins
over any wording here.

**Endpoints.** All require a session, except as noted in hosting §Security 1. Every mutation
takes an idempotency key and, for edits, the expected `version`.

| Area | Endpoints |
|---|---|
| Auth | sign in, sign out, join (code), enrol screen, revoke, re-issue codes, `whoami` |
| Events / shows | active show, show mode, songs (reorder after current, skip), practice copy, compare / apply-to, print views |
| Devices | CRUD, guest device, status (suspect / out / missing / found) |
| Plot | cells (set, fill right, clear), passes CRUD, pass done |
| Holdings | handoff Done (give / collect / pass / capsule), Leave it, count-in, close count-in |
| Substitutions | swap (scope), pull, in hand, FOH confirm, reverse, return, rest-of-show |
| Alerts | report, on it, fixed, snooze, re-point to TX |
| Battery | check, line check, section done |
| Checklists, messages | tick / untick, send, ack |
| People | photo upload, zones, absences |
| Live | snapshot, `/api/ws`, presence, `/api/midi` (agent key, with `cued_ms_ago`) |
| Admin | health detail, backups (list, now, restore from local / R2 / file), export, takeover (standby) |

**WebSocket messages:** `hello {seq, server_time, epoch}`, `event`, `resync`,
`tick {seq, server_time, epoch}`.
- Clients never notify for events with seq ≤ the hello seq, except still-NEW criticals they own
  (once).
- A new epoch triggers acked-command replay (reliability §3.3).

---

## 20. Acceptance tests

Automated in Rust and Playwright unless marked (manual). Thresholds are pass/fail.

1. **Speed.**
   - Raise → visible on 3 other clients: p95 < 1 s over 200 trials, against the deployed URL.
   - Swap → FOH card: p95 < 1 s over 200 trials.
   - Tap counts: Batt OK 1 tap in battery service; swap 3 taps from an alert; report 2 taps from
     a FOH row.
2. **Stable layout.** Tile bounding boxes are identical (±0 px) across every song change and every
   alert state in the seeded show.
3. **No status colour when healthy.** A pixel scan of the healthy state finds no pixel within
   ΔE2000 < 10 of status red or amber, excluding chip stripes.
4. **Colour-blind.** Every state carries a word and glyph in the DOM. Greyscale and simulated
   deuteranopia/protanopia screenshots are reviewed (manual checklist).
5. **Replay.** A fresh load with 50 historic events never toasts, vibrates, pushes or pulses. A
   still-NEW owned critical notifies exactly once.
6. **Grouping.** 28 rule alerts produce ≤ 5 rows plus a group row.
7. **Passes.** RX10 pass during *I Will Follow Jesus*:
   - before Pass done, the holder is Helen and an alert raised on RX10 shows Helen;
   - after Pass done, it is Shosh;
   - at the next song, "Pass RX10: Shosh → Helen" is the only RX10 handoff.
8. **Jumps.** MIDI 2 → 4:
   - the Now list equals planned(4) vs holdings;
   - no song-3 cards;
   - jumped passes cancelled;
   - no songs marked skipped.

   Going back 4 → 3 lists the reverse moves and changes nothing until Done.
9. **Reorder.** Reordering songs after the current one recomputes handoffs, keeps PC numbers and
   produces per-A2 diff cards. Reordering the current or earlier songs is refused.
10. **Carry-over.** Afternoon → evening with the same performer shows "Keep with Helen for
    evening" and no collect/give pair.
11. **Rest of show.** "Replace Red with Tan for rest of show" moves every later Red cell to Tan.
    A Tan clash at Song 5 raises a WARNING at Song 4.
12. **Pull.** Pull Red: the tile shows Pulled, FOH gets a mute card, and Red raises no battery
    alerts.
13. **Evidence window.** An afternoon check shows "checked for earlier show" in the evening and
    doesn't satisfy battery-not-checked.
14. **Offline queue.** Phone offline → Batt OK and Done queued with a badge → reload while offline
    shows the shell + OFFLINE → reconnect → applied once, with the original song.
15. **Epoch replay.** Restore a backup missing 3 acked commands → clients replay → 2 apply and 1
    conflicts (listed once).
16. **Swap flow.**
    - Recorded → In hand → FOH Done;
    - pack-only wording;
    - undo before In hand, reverse after it;
    - FOH-confirm-pending when every venue screen is offline.
17. **Guest device.** Add Chandler's pack replacing RX9: RX9 is Done with no battery alert;
    Chandler's later RX9 cells move.
18. **Count-in.** Close count-in marks outstanding devices Missing with the last holder and song.
19. **Practice isolation.** Actions in a practice copy change no real holdings, checks, alerts or
    equipment status. A MIDI cue doesn't move it.
20. **Auth.**
    - Every endpoint except the listed public ones returns 401 without a session (GET and the WS
      included).
    - A join code works on 3 devices, then fails; it fails after 2 h.
    - 5 wrong PINs lock that name on that device (429), while another device still signs in.
    - 20 wrong PINs from rotating device ids pause the name on new devices; a device that signed
      in before still gets in.
    - 20 wrong join codes in 5 min pause joining.
    - A screen credential survives 8 days while in show mode.
    - Revocation takes effect on the next request.
21. **Headers.** CSP, HSTS, `frame-ancestors`, Origin rejection on a cross-site WS upgrade.
22. **Touch.** Wall targets ≥ 64 px; phone primary ≥ 48 px; Done/Problem ≥ 64 px (DOM measurement).
23. **Contrast.** Text ≥ 4.5:1; wall primary and alert ≥ 7:1 (automated contrast audit).
24. **Reliability.** All requirements in `docs/reliability.md` §4.
25. **Migration.** The current HF 2026 database migrates to v3.1 (`docs/data-model.md` §5):
    - every performer and song without a pass keeps an identical effective assignment;
    - Helen/Shosh becomes one pass, with phase-0 holder Helen and post-pass holder Shosh (the
      old effective holder);
    - the migration report lists old rest-of-show swaps and per-kit mix differences.
26. **Volunteer test (manual).** A first-time helper, with the 3-card intro only, completes:
    battery service for their section, a give, a pass, and a Problem report, without help.
27. **Crew review (manual).** Two A2s and the FOH engineer run the practice show before the first
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
| Volunteer card | §1.1 3-card intro + §20.26 |
| Message read receipts | §17 |
| Not here tonight | §15.4 |
| Hardware: TX_DEVICE_ID not on ULXD1/2 | Wrong-pack check is manual (preset); `docs/hardware-integration.md` §1 |
| Hardware: Playback fires on play only | §4.2, hardware §5 (cue at 0:00 of each song) |

---

## 22. Build order

1. **Reliability foundation** [done, server side]: DB pragmas, verified/pinned backups, startup
   recovery, heartbeat tick, epoch, presence, health, SIGTERM.
   - **Client side:** STALE / OFFLINE / paper states, resume, server-time offset, no alarms for
     replayed history.
   - **Retention:** 20 local automatic backups and free-space percentages.
1b. **Security and cloud:**
   - signed cookie sessions and sign-in on everything;
   - mandatory PINs and the first-admin env/CLI;
   - join codes and screen credentials;
   - per-name/device rate limits, Origin checks, headers and CSP;
   - hashed agent key;
   - R2 upload, generation-prefixed keys and the fencing lease;
   - `render.yaml` and the Cloudflare Worker proxy.
2. **Data model v3.1:**
   - Tables: devices, show bases (mix), plot cells, visits, passes + marks, substitutions (all
     kinds), holdings, events/time zone, evidence window, practice scope.
   - Migration 0006 from today's schema, with its report.
   - Core functions: planned (phases), handoffs, usage, unusable devices, conflicts, after show.
   - Tests: every scenario in the third review (a–l) and §20.7–20.13, §20.25.
3. **Shared UI:** status bar, song strip (hold-to-advance), activity line + undo, alert
   column/strip + critical band, device tile, device panel, problem presets.
4. **Wall board,** next handoffs and battery service.
5. **Swap** (two-step, pull, rest of show), FOH channels and swap cards.
6. **Phone tabs,** offline queue + acked log, plan-diff cards, onboarding (join, intro), and
   Web Push.
7. **Admin:**
   - plot (passes, fill right);
   - devices;
   - show (day/set, print, compare/apply, practice);
   - people (photos, zones, absent);
   - operators and screens.
8. **Checklists,** count-in / missing, quick messages, report.
9. **Full acceptance suite** (§20) + reliability tests + soak + screenshots for crew review.
10. **Deploy** to Render with the owner (hosting §Owner setup steps), plus the restore and standby
    drills.

Then Phase 5 telemetry attaches to the tile's battery/RF row and the §7.6 rules, with no layout
changes.

---

## 23. v2 → v3 change log (second independent review)

### Blockers (all resolved)

| # | Finding | Resolution |
|---|---|---|
| 1 | Devices were per-show kits | Global `devices` + `show_devices` overrides; migration 0005 step 3 (data model §2, §5) |
| 2 | Person/kind override model couldn't express the plot | `plot_cells` device × song + Default; passes table; migration steps 4–5 |
| 3 | Handoffs stored as ticks went stale on reorder/jump | **Holdings** are physical truth; handoffs are computed differences (data model §3.4) |
| 4 | Failed device used later in the show | Swap scope **Rest of show**; "free until Song N" + clash WARNING one song ahead (§10.2–10.3) |
| 5 | Pull undefined; spare capsules; "Tell FOH" to FOH | Pull = substitution with no replacement, tile "Pulled — mute Ch N", FOH card; capsule count; Tell FOH hidden for FOH (§10.2) |
| 6 | iOS Home-Screen cookie isolation broke join | Code works for that helper on 3 devices for 2 h, with manual 6-digit entry (§1.1) |
| 7 | Join code length/lifetime inconsistent | 6 digits, per named helper, 2 h; kiosk QR 60 s after Lead PIN; re-issue all (§1.1, hosting §Security 3) |
| 8 | Wall/FOH sessions expire mid-show; no attribution | Screen credentials (7 days, sliding, never expire in show mode, revocable); "Wall (Sam)" (§1.1) |
| 9 | Per-IP lockout locks out the whole venue | Per name + device limits; IP cap only alerts (hosting §Security 6) |
| 10 | "Any name" bootstrap on the public internet | Removed; first Lead from env/CLI; PIN-less operators can't write (hosting §Security 2) |
| 11 | Practice show could touch real data | Scope on holdings, checks, alerts, substitutions; MIDI ignored; never active (data model §4) |
| 12 | Venue internet down, phones on cellular | "Venue screens offline" banner, FOH-confirm-pending, one NOTE (reliability §3.4) |
| 13 | Cues lost while the agent was disconnected | Last-cue buffer; < 10 s applies, else Song check (reliability §3.5) |
| 14 | Standby had split-brain risk and logged everyone out | Cloudflare Worker origin flip + R2 fencing lease + shared signing key + client replay (reliability §3.3, §3.6; revised in §24 B8) |
| 15 | Retention didn't fit the disk | 5 GB disk; 20 local automatic backups + 30-day pinned; R2 long-term; 25 % / 10 % thresholds (reliability §2.2) |
| 16 | Times without a time zone | `events.timezone`; UTC storage; display in event zone (data model §2) |
| 17 | Battery checks from the afternoon counted for the evening | Evidence valid only from call − 2 h (§8.1) |

### Other findings (decisions adopted)

| # | Decision | Where |
|---|---|---|
| 18 | Parallel battery taps; undo only by the same operator within 10 s; device zone rule; Line ✓ column | §8.2 |
| 19 | One "Done" verb; "Collect from X" usage | §5.1, §9.1 |
| 20 | Pass routing (giver's A2 card, receiver's info); prep one song ahead; return is a normal handoff | §9.2–9.3 |
| 21 | Jumped-over passes cancelled; departed-song passes overdue only if still different; MIDI jumps unconfirmed | §9.4 |
| 22 | Reorder only after the current song; FOH diff cards | §13 |
| 23 | Guest replace moves the remaining cells; show-scoped labels | §13 |
| 24 | Helper permissions | §1 table |
| 25 | Swap marks *suspect*; out of service is Lead-only; other form factor as a third tier | §10.2–10.3 |
| 26 | STALE 25 s / OFFLINE 60 s / paper 120 s; device vs server wording | §11, reliability §3.1 |
| 27 | Service worker shell only; IndexedDB queue | §11 |
| 28 | Web Push via VAPID; HTTP branch dropped | §7.5 |
| 29 | Presence severity = NOTE | §7.6, reliability req 20 |
| 30 | "Keep with X for evening" changeover | §12.2 |
| 31 | Count-in from holdings; "Close count-in" → Missing | §12.3 |
| 32–33 | Alerts, swaps and shows migrations | data model §5 steps 7–8 |
| 34 | CSP `style-src 'unsafe-inline'` | hosting §Security 10 |
| 35 | Health detail Lead-only | hosting §Security 7 |
| 36 | Stale LAN / LaunchDaemon / HTTP text purged | reliability (rewritten), spec, hosting. Operations is flagged as dev/rehearsal-only until step 10; README auth text changes with step 1b |
| 37 | Retention conflict | reliability §2.2 |
| 38 | Cellular survey per venue | reliability §5 Phones |
| 39 | Lenient Render health check | reliability req 9 |
| 40 | Verb labels and hint lines | §9.1 |
| 41 | dLive "copy processing" line | §10.5 |
| 42 | Measurable tests | §20 |

---

## 24. v3 → v3.1 change log (third independent review)

### Blockers (all resolved)

| # | Finding | Resolution |
|---|---|---|
| B1 | The Helen/Shosh migration contradicted its own test | Passes are made before cells, and pass pairs are skipped; the test checks phase-0 and post-pass holders (data model §5 steps 4–5, §20.25) |
| B2 | Substitutions and passes didn't compose (failed RX10 handed to Shosh; a post-pass swap did nothing) | Phases per song; a substitution moves the device's whole pass chain in that song, matching any phase holder (data model §3.1) |
| B3 | Pass state was per song, not per visit | `song_visits` + `pass_marks(pass, visit)`; going back replays passes; jumped passes are derived, not stored (data model §2.3, §2.5, §3.5) |
| B4 | The swap and the handoff list both wrote the same holding | Substitution steps own the holdings (In hand / Return); their devices are left out of the handoff lists while recorded (data model §2.6; §10.5) |
| B5 | A rest-of-show replacement overwrote another performer's cell | Overlay never overwrites; conflict instead. A2 **reassign** substitution resolves it without a plot edit (data model §3.1, §3.7) |
| B6 | A suspect device was still handed to its next planned holder | Unusable devices are never handed out; one-song-ahead conflict with Extend / Pick spare / Mark fixed (data model §3.6; §10.6) |
| B7 | TX failure couldn't be expressed; pack swaps silently changed the mix | `tx_swap` substitution; retune option; mix belongs to the TX (`show_bases`); multi-holder alerts and routing (data model §2.2, §2.6, §3.10; §7.4, §10.2) |
| B8 | Standby could fence the good server; epoch ambiguity; Render domain move | Standby always starts fenced; lease generation separate from the epoch; If-Match takeover; fenced services stop uploads; generation-prefixed backups; Worker origin flip; takeover checks the PIN against the downloaded backup; runbook suspends the old service first (reliability §3.6) |
| B9 | Brute force by rotating device ids; join codes unlimited | Per-name limit across devices + trusted-device budget; global join budget 20 / 5 min; limits before argon2, 2 concurrent hashes. Built and tested (`tests/security.rs`) |

### Decisions adopted

| # | Decision | Where |
|---|---|---|
| 1 | Migration mappings (status, day, NULL call, nullable base, Lead label, holdings via old handoff plan, old rest-of-show flagged) | data model §5 |
| 2 | If no Lead has a PIN after migration or restore, the env admin is upserted | data model §5 step 10 |
| 3 | Departed-visit passes aren't carried over; Now covers them | data model §3.5 |
| 4 | Guest: performer = the guest; event-scoped guest labels; holder "owner" | data model §2.2, §2.6, §2.7 |
| 5 | After-show target = the linked same-day show's first song; "Keep with X"; count-in excludes kept devices; pre-show Now is empty | data model §3.3, §3.9 |
| 6 | Evidence window = max(call − 2 h, the previous linked show's end) | data model §2.9 |
| 7 | Pull: Pulled at once; the collect waits for the song boundary | data model §2.6; §10.5 |
| 8 | Practice: per-scope equipment status; Playback only when no real show is live; no push; doesn't keep sessions alive; included in backups | data model §4 |
| 9 | Show-mode keep-alive only for cookies valid at show start, with absolute caps; sockets re-check every 10 s | data model §2.10 (built) |
| 10 | Backup + R2 upload immediately after any sign-in change; takeover verifies the PIN against the downloaded backup | reliability §3.6 (backup built) |
| 11 | Worker proxy instead of a Render domain move; suspend the old service first | reliability §3.6, hosting |
| 12 | Persistent cookies; re-admit by a new code; 3 uses explained | data model §2.10, hosting §Security 3 |
| 13 | Screens can never hold the Lead role | data model §2.10, hosting §Security 4 (built) |
