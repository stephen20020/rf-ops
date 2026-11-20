# RF Ops data model (v3.1, authoritative)

This document is the single source of truth for what RF Ops stores and how holders, handoffs and
usage are worked out.
- **v3** resolved the second review's blockers 1–5, 11, 16–17 and decisions 19–23, 30–33.
- **v3.1** resolves the third review's blockers B1–B8 and its decisions.
- Both are logged in `docs/interface-spec.md` §23–24.

## 1. Core idea

**Devices** (Red, Bpack2, RX10, GLX 1) are permanent and shared by every show. Each show has a
**plot**: a grid of device × song, with a person in each cell. Separately, RF Ops always knows
**who physically has each device right now** (the *holding*).

**A handoff is any difference between "who should have it" and "who has it".**
- Doing the handoff updates the holding.
- Nothing else is ticked or stored, so reorders, MIDI jumps, going back, changeovers and count-in
  all fall out of the same comparison.

There are **two exceptions**, where a flow owns the holding instead of the comparison:
1. **Substitutions** (swap, pull, return) move holdings through their own steps (§2.6).
2. **Unusable devices** are never handed to anyone (§3.6).

## 2. Tables

### 2.1 Physical inventory

**`equipment`:** one row per physical item: receivers, receiver channels, transmitters, capsules,
headsets, IEM TX channels, IEM packs, guitar receivers/packs, rechargeable batteries.

| Field | Values |
|---|---|
| `battery_kind` | aa_alkaline \| aa_nimh \| aa_lithium \| sb900 \| sb902 \| none |
| `owner_performer_id` / `owner_text` | NULL = house |
| `telemetry` | set of: battery_bars, runtime, rf, tx_link; or none |
| `status` | available \| suspect \| out_of_service \| missing |
| `active` | false = retired |

`label` is unique among house items. Guest items are unique per event (§2.2).

### 2.2 Devices (the board's tiles)

**`devices`** (global):
- `id`, `label` (Red), `kind` (mic | iem | guitar)
- `carrier_id` → equipment: transmitter / IEM pack / guitar pack. Unique among active devices.
- `base_id` → equipment, **nullable** (unknown yet; the TBC flag is shown):
  - mic: receiver channel, unique among active mic devices;
  - iem: TX channel, **may be shared** (RX1, RX3, RX6 on TX1);
  - guitar: receiver, unique.
- `element_id` → default capsule / headset / lav (nullable)
- `console_channel`, `console_name`
- `section` (mics | iem | guitar | spare | guest), `board_order`
- `zone_override` (nullable)
- `is_spare`, `active`
- `guest_event_id` (nullable). A guest device exists for one event; its label is unique within
  that event ("Chandler's pack").
- `version` (optimistic concurrency)

**`show_devices`** (per-show overrides): `show_id`, `device_id`, `tbc` (JSON flags), `excluded`.

**`show_bases`:** `show_id`, `base_id` (an IEM TX), `mix` ("ME 3", "Console mix").
- **The mix belongs to the TX, not the pack.** A pack's mix in a song is the mix of its
  *effective* TX (§3.1). Moving a pack to another TX therefore changes what the performer hears,
  and the FOH card says so.

### 2.3 Events, shows, songs, visits

**`events`:**
- `id`, `name` ("Homestead Fair 2026")
- **`timezone` (IANA, e.g. America/Chicago)**
- `starts_on`, `ends_on`

**`shows`:**
- `id`, `event_id`, `name`, **`day`** (local date), `set_name` (afternoon | evening | …)
- `concert_at`, `call_at`: UTC instants, nullable. Entered and shown in the event time zone. Call
  defaults to concert − 1 h.
- `mode`, `midi_mode`, `current_visit_id`
- `activated_at`, `ended_at`: when it became active and when show mode last ended.
- **`is_active`:** a partial unique index allows only one active row.
- `practice_of` (nullable)
- `linked_prev_show_id` (the afternoon show for an evening; Friday for Saturday)

**`songs`:**
- PC number bound to the song.
- `skipped`: a persistent Lead decision. A jump never sets it.

**`song_visits`:**
- `id`, `show_id`, `song_id`, `visit_no` (1, 2, … within the show), `started_at`, `via`
  (manual | midi | jump | back)
- Every song change starts a new visit. The current song is the current visit's song.
- **Going back to Song 3 is a new visit of Song 3.** Its passes are planned again.

### 2.4 The plot

**`plot_cells`:**
- Primary key `(show_id, device_id, song_id)`. `song_id` NULL = the **Default** column.
- `performer_id`: NULL = nobody.
- `element_id`: NULL = the device's default element. Lets a cell say "Red with KSM9".

**Rule:** cell(S), else cell(Default), else nobody.

### 2.5 Passes (person-to-person changes during a song)

**`passes`** (the plan):
- `id`, `show_id`, `device_id`, `song_id`, `seq` (order within that device and song), `to_performer`
- The *from* side is implied: the holder before this pass (the cell for `seq` 1, else the previous
  pass's `to`).

**`pass_marks`:** `pass_id`, `visit_id`, `by`, `at`. One row means "done in this visit".
- Nothing else is stored.
- A pass in a song that was jumped over simply has no visit.
- A pass in a departed visit that wasn't done stays undone in that visit. The Now comparison of
  the new song covers whatever physically still needs to move.

**Example:** Helen → Shosh on RX10 in *I Will Follow Jesus* is stored as:
- RX10 plot cell (Default) = Helen;
- one pass (RX10, IWFJ, seq 1, to Shosh).

The return at the next song is an ordinary handoff, because the plot says Helen and the holding
says Shosh.

### 2.6 Substitutions (swap, pull, reassign, guest, TX swap)

**`substitutions`:**

| Field | Meaning |
|---|---|
| `id`, `show_id`, `scope_id` | `scope_id` = real, or a practice show |
| `kind` | swap \| pull \| reassign \| guest \| tx_swap |
| `performer_id` | Whose device changes. NULL = every holder (rest of show, TX swap) |
| `failed_device_id` | The device taken away (NULL for tx_swap) |
| `replacement_device_id` | NULL = pull |
| `failed_base_id`, `replacement_base_id` | tx_swap: TX1 → TX9 for every pack on TX1. swap: `replacement_base_id` = the spare pack is **retuned** to the failed pack's TX |
| `from_song_id`, `to_song_id` | `to` NULL = open-ended |
| `range` | this_song \| until_fixed \| rest_of_show |
| `state` | recorded → in_hand → foh_confirmed; or → reversed; later return_recorded → returned |
| `failed_component` | unknown \| carrier \| element \| channel |
| who / when | for each state change |

**Who may create which kind:**
- swap, pull and reassign: any A2.
- guest: A2 or Lead.
- tx_swap: Lead or FOH.

**Kinds:**
- **swap:** P moves from F to R. Marks F's chosen component suspect.
- **pull:** P loses F with no replacement.
- **reassign:** P moves from device A to device B with nothing broken. It is how an A2 resolves a
  conflict without a plot edit (§3.7).
- **guest:** P (the guest's own performer row, e.g. Chandler) moves from F (RX9) to the guest
  device. Only P's cells move.
- **tx_swap:** every device whose base is F_base uses R_base instead, for the range.

**Holdings are owned by the substitution's steps, not by the handoff comparison:**

| Step | Holding change |
|---|---|
| Recorded | none |
| **In hand** | R → P (`via` swap). F → stock: the A2 took it back |
| Pull, In hand | F → stock at the **next song boundary**, not mid-song. The performer stays on stage; the tile reads "Pulled — mute Ch 34" at once |
| **Return** (until_fixed) | Its own Recorded → In hand: F → P, R → stock. Then `to_song_id` = the song before the return |

While a substitution is `recorded` (or `return_recorded`), its devices F and R are left out of
the Now and Coming-up handoff lists. The swap card is the only instruction.

### 2.7 Holdings (physical truth)

**`holdings`:** one current row per (scope, device).
- `holder_kind`: stock | performer | owner (a guest device given back to its owner)
- `holder_performer_id`, `element_id` (capsule/headset fitted now)
- `since`, `by`
- `via`: give | collect | pass | swap | pull | return | count_in | manual | keep
- History lives in `event_log` (`holding.changed`).

Holdings are global (per scope), so they carry over between shows (§3.9).

### 2.8 Alerts

**`alerts`** add:
- `type` (interface spec §7.6), `priority`
- `device_id`, or `equipment_id` for a TX/receiver-level alert. One of the two is required for
  device problems.
- **`holders_at_raise`** (JSON array: a TX alert has several), `song_at_raise`, `failed_component`
- `repeat_count`, `snoozed_until_visit_id`, `owner_operator_id`, `on_it_by`, `on_it_at`
- `scope_id`

### 2.9 Battery checks

**`battery_checks`** add: `device_id`, `battery_kind`, `line_check` (bool), `scope_id`.

**Evidence window for show S.** A check counts if it was recorded at or after:
```
start(S) = max( S.call_at − 2 h        (or S.activated_at if call_at is NULL),
                prev.ended_at          if S has a linked previous show that has ended )
```
So with a one-hour afternoon→evening changeover, afternoon checks never count for the evening.

| Check recorded | Shown in show S |
|---|---|
| Before start(S), same day | amber "checked for earlier show (13:02)" |
| Another day | "✓ Fri 19:02", amber from call |

### 2.10 Operators, sessions, screens (built in step 1b)

**`users`:**
- `pin_hash` nullable. Only helpers and not-yet-set-up operators lack one, and those can't sign in
  by PIN.
- `role`: admin (shown as **Lead**) | a2 | foh | helper | viewer
- `zone`, `sessions_valid_after`

**`screens`:** `id`, `name` (Wall SL, FOH), `role` (a2 | foh | viewer, **never admin**),
`enrolled_by/at`, `last_seen_at`, `revoked`.

**Sessions** are signed, stateless cookies (HMAC-SHA256, `RFOPS_SESSION_KEY`).
- **Claims:** sid, kind (user / screen), id, sign-in time (ms), issue time, expiry.
- **Lifetime:** users 18 h, screens 7 days, sliding (re-issued after 1 h). The cookie is
  persistent (`Max-Age`), because iOS Home-Screen apps drop session cookies.
- **Show mode:** a cookie that was still valid when the show entered show mode
  (`settings.show_mode.since`) stays accepted until show mode ends. There is an absolute cap of
  7 days from sign-in for users and 30 days for screens. Practice shows don't count.
- **Revocation:** `session_revocations(sid)`, `users.sessions_valid_after` (a PIN change or "sign
  out everywhere"), and `screens.revoked`. Open WebSockets re-check the session every 10 s and
  close.

**`join_codes`:**
- `code_hash` (HMAC, not the code), `user_id` (a named helper), `uses_left` 3, `expires_at`
  (2 h), `created_by`.
- A new code for a helper replaces the old one ("re-admit"). Scanning in Safari and then adding
  the Home-Screen icon uses 2 of the 3 uses.

**Device cookie** `rfops_device`: a signed marker on a browser that has signed in as a name
before. During a name lockout that browser keeps its own budget (§Security 6 in hosting).

**`push_subscriptions`:** `user_id` / `screen_id`, endpoint, keys.

### 2.11 Other tables

- `zones` (managed list), `performers.zone_id`, `performer_absences(show_id, performer_id)`
- `checklists` / `checklist_items` / `checklist_ticks`
- `messages` (phrase, device, from, to, urgent, acked)
- `practice_equipment_status(scope_id, equipment_id, status)`
- `settings` (epoch, keys, show_mode.since)

## 3. Derived values (pure functions in `rfops-core`)

### 3.1 Planned holder: `planned(show, device, song, phase)`

**Phase** k = the number of that device's passes in the song marked done in the current visit
(for other songs, k = 0 at the start of the song, and the last phase at its end).

1. **Plot and passes:**
   - h(0) = the cell (song, else Default, else nobody);
   - h(k) = the `to` of pass k.
2. **Substitutions,** in creation order. Each applies to songs in [from, to]. Let F be the failed
   device and R the replacement.
   - **Match.** It matches device F in a song if P (or anyone, when `performer_id` is NULL) is
     any phase holder h(k) of F in that song. Then **the whole pass chain of F in that song
     moves to R**: each phase holder of F becomes R's phase holder, and F becomes nobody.
     - If Helen's RX10 is swapped to RX9, the RX10 pass Helen → Shosh is carried out on RX9.
     - A swap made for Shosh after Pass done matches too.
   - **No overwriting.** If R has its own non-nobody planned holder in that song (before any
     overlay), the overlay does **not** apply in that song. The result is a **conflict** (§3.7),
     never a silently dropped person.
   - **pull:** F becomes nobody for P; there is no R.
   - **tx_swap:** no holders change; the effective base of every device on F_base becomes R_base.
3. **Effective base** of a device in a song:
   - its `base_id`;
   - then any tx_swap;
   - then a swap that **retuned** this device (`replacement_base_id`).
4. **Mix** of an IEM device in a song = `show_bases.mix` of its effective base.

### 3.2 Physical holder

`physical(device)` = the holdings row in the current scope.

### 3.3 Targets

| Target | Definition |
|---|---|
| **Now** | Current song, at the current phase |
| **Coming up** | Next non-skipped song, phase 0, or **after show** (§3.9) |
| **During this song** | Undone passes of the current song in this visit, in `seq` order |
| **Pre-show** | No current visit: Now is empty, Coming up = the first song |

### 3.4 Handoffs: `handoffs(show, target)`

For each device in the show, compare `physical(device)` with `planned(device, target)`.

**Skip** a device when any of these holds:
- it is in a substitution that is recorded or return_recorded (§2.6);
- the target would hand it **to** someone while its carrier is unusable (§3.6).

| Physical → planned | Handoff |
|---|---|
| stock → P | **give** |
| P → stock (nobody, including after show) | **collect** |
| P1 → P2 | **pass** |
| same holder, different element | **capsule swap** |
| holder_kind owner → anyone | none (gone home) |

Handoff identity, used by the UI only and recomputed every time: (device, type, to-holder,
target). Doing it writes a holding change; nothing else is stored.

### 3.5 Overdue, coming up, jumps, going back

- **Overdue:** handoffs against **Now**. Each gets a WARNING on the device, with **Done now** or
  **Leave it** (an exception recorded for this visit only).
- **Coming up:** handoffs against Coming up, plus the next song's passes as a prep card one song
  ahead.
- **Jump 2 → 4:**
  - A new visit of Song 4 starts.
  - Now = planned(4, phase 0) vs physical: exactly the combined change. Song 3's handoffs and
    passes never appear.
  - Undone passes of the departed visit are not carried over; Now covers what still needs to
    move.
  - A jump never sets `skipped`. Going back to Song 3 later starts a fresh visit with its passes
    planned again.
- **Going back 4 → 3:** a new visit of Song 3, with the same comparison. If Shosh still holds
  RX10 from the earlier visit, Now shows "Pass RX10: Shosh → Helen", which is correct: Helen needs
  it at the start of Song 3. Nothing is undone automatically.

### 3.6 Unusable devices

A carrier that is suspect, out_of_service or missing (per scope) is **unusable**.
- No give, pass or capsule handoff is generated **to** a holder of an unusable device. Collects
  from its holder still are.
- Instead, one song before each later planned use **not already covered by a substitution**, a
  conflict WARNING:

  > "Red is suspect but planned for Judah at Song 6." [Extend the swap to Judah] [Pick a spare]
  > [Mark Red fixed]

  **Extend** creates a swap for Judah with the same replacement if that device is free in those
  songs, otherwise it opens the swap panel.
- This covers:
  - the default until_fixed scope;
  - an expired this_song swap;
  - a suspect device carried into the evening show.

### 3.7 Conflicts

Computed for each song **on the plot before substitutions**, and again for each substitution's
effect.

| Level | Conflict |
|---|---|
| **Error** | The same performer holds two IEM packs, or two guitar packs |
| **Error** | The same element is used by two devices |
| **Error** | A replacement R is planned for X in a song the substitution covers. Shown one song ahead as "Tan is needed by X at Song 5, and is replacing Red for Judah" with [Pick another for Judah] [Give X a different mic] [Tell FOH] |
| **Warning** | A performer holds two mics (allowed, but flagged) |
| **Warning** | A planned holder is marked absent |
| **Warning** | An unusable device is planned (§3.6) |

- **[Give X a different mic]** creates a **reassign** substitution, which A2s may do. No plot edit
  is needed.
- Device-level rules prevent two active mic devices sharing a receiver channel. IEM TX sharing is
  allowed.

### 3.8 Usage

Usage of device D at the current song C:

| Condition | Usage |
|---|---|
| Carrier unusable (out / suspect) or missing | **out** / **suspect** / **missing** |
| Failed device of an active pull | **pulled** |
| Replacement in an active substitution | **on spare for <failed>** |
| planned(D, C, current phase) ≠ nobody | **in use** |
| Nobody now or later, physical = someone | **collect from <holder>** |
| Nobody now, planned in a later song | **next** |
| `is_spare` and unplanned | **spare** |
| `guest_event_id` set | **guest** |
| Nobody now or later, stock | **done** |

**Precedence:** out / missing > suspect > pulled > on spare > in use > collect > next > spare >
done. Guest is a section and label, not a usage.

### 3.9 After show, carry-over, count-in

The **after show** target for device D in show S:
- If a show T has `linked_prev_show_id` = S and the same `day`, the target is
  planned(T, T's first non-skipped song, phase 0).
- Otherwise the target is nobody.

**Consequences:**
- **Keep with Helen for evening:** physical = Helen and target = Helen. There is no handoff, and
  the card reads "Keep RX10 with Helen for evening".
- **Count-in** at the end of S lists devices whose holding isn't stock, **excluding kept ones**.
  Guest devices show "Return to owner" (→ holder_kind owner).
- **Close count-in** (Lead) marks the rest **missing**, keeping the last holder.

### 3.10 Device zone

`zone_override`, else the zone of the device's first planned holder in this show, else none. Used
for battery-service filters and alert ownership.
- A TX-level alert is routed to the zones of **all** current holders of packs on that TX.

## 4. Practice shows

**Creating one:** "Practice show" copies a show (plot, passes) with `practice_of` set and its own
`scope_id`.

**Everything written while viewing it carries that scope:**
- holdings (start from stock);
- battery checks;
- alerts;
- substitutions;
- pass marks;
- visits;
- equipment status (`practice_equipment_status`).

**Never affects the real show:**
- It can never become active.
- Its show mode doesn't keep sessions alive and doesn't set `show_mode.since`.
- MIDI is ignored. "Practice with Playback" can be turned on only while no real show is in show
  mode; cues then move only the practice show.
- No push notifications.
- Excluded from reports and evidence. Practice rows *are* in backups, which copy the whole
  database.

**Look:** "PRACTICE" watermark on every screen.

## 5. Migration from 0001–0005

Run as migration `0006_v3_model.sql` plus a Rust data step, both in one transaction, after a
pinned "pre-migration" backup. It writes a **migration report** (shown in Admin → Data and kept
in the event log).

1. **Events.** One event, "Default event", with its time zone from `RFOPS_TZ` (default
   America/Chicago). Every show is attached.
2. **Shows.**
   - `starts_at` → `concert_at`; `call_time` → `call_at`. Both may stay NULL.
   - `day` = local date of `concert_at`, else of the show's first event_log row, else the
     migration date.
   - The show in `mode='show'`, else the highest id, becomes active (`activated_at` = now).
   - `current_song_id` → one visit (`via` manual).
3. **Devices.** For each distinct (kind, carrier_id) across `kits`, create one device.
   - Label = the carrier's label. The base comes from the first kit, and **may be NULL** (TBC:
     Gabe's Shure, Sennheiser 1 and Sennheiser 2 in HF 2026).
   - `console_channel` / `console_name` from the first kit; `element_id` = the most common
     element.
   - Kits for the same carrier that differ only by element (Lead Vox / Lead Vox KSM9) collapse
     into one device.
   - Kit mix labels become `show_bases.mix` per show and TX. A per-kit mix that differs from its
     TX's mix goes in the report.
   - Equipment status: available / checked_out / in_use → available; faulty → out_of_service;
     retired → `active` false.
4. **Passes first.** A song override pair that moves one device from A to B *within the same
   song* (A's override drops the device, B's adds it) becomes a pass, with the cell for that
   song = A. This is the Helen / Shosh pattern. Mark those (device, song) pairs as done.
5. **Plot cells.**
   - The Default column comes from each performer's defaults, mapped kit → device.
   - For every song, compute the old effective assignment and write a cell only where it differs
     from Default, **skipping pairs made into passes in step 4**.
   - An element difference → the cell's `element_id`.
6. **Holdings.**
   - Re-run the old `handoff_plan` for the active show to recover performers (`handoff_done` has
     none).
   - Apply its done rows in order: issue → give, return → collect, transfer → pass,
     component_change → capsule.
   - Then apply open `checkouts`. The −1 sentinels mean before / after the show.
   - Drop `handoff_done`.
7. **Swaps → substitutions.**
   - Old `song` → this_song.
   - Old `rest_of_show` → until_fixed, performer = the swapped performer. The old meaning was
     "this person, rest of show", not v3's "every holder". Each is listed in the report.
   - All historical swaps get state `foh_confirmed`.
8. **Alerts.**
   - severity → priority.
   - `type` from the dedupe_key prefix: `battery:` → battery_not_checked; `midi-unmatched:` →
     midi_no_song; `midi-bridge:` → bridge_offline; `auth:` → security; anything else → other.
   - State: open → NEW; acknowledged → ON IT; resolved → FIXED, or cleared if
     `resolved_by='system'`.
   - `performer_id` → `holders_at_raise[0]`.
9. **People and checks.**
   - Zone strings become `zones` rows.
   - `battery_checks.device_id` comes from carrier → device.
10. **Operators.**
    - If no Lead has a PIN after migration **or after any restore**, the `RFOPS_ADMIN_NAME` user
      is upserted with `RFOPS_ADMIN_PIN`. This prevents a lock-out after restoring a file onto
      Render.
    - Operators without a PIN can't sign in until the Lead sets one.
    - The old `kits` and `assignments` tables are kept read-only for one release, then dropped.

**Migration test** (interface spec §20.25):
- For every performer and song *without* a pass: the effective assignment is identical.
- For pass songs: holder at phase 0 = the pass's from-performer, and the holder after the pass
  = the old effective holder.

**Moving data to Render:** take a verified local backup, then Admin → Data → **Restore from
file** (Lead + PIN) on the new service. That runs the same migrations.

## 6. Invariants (enforced and tested)

1. At most one active show.
2. A carrier is unique among active devices. A mic channel / guitar receiver is unique among
   active devices of that kind.
3. A plot cell refers to a device, a song of its show, and an existing performer.
4. Passes of a device in a song have consecutive `seq` and are marked in order.
5. A holding refers to an active device. A guest device is stock or with its owner before its
   event is archived.
6. A replacement is of the same kind. Handheld ↔ bodypack is allowed and flagged.
7. At most one non-final substitution per (failed device, performer) at a time.
8. Every state change writes an `event_log` row in the same transaction; practice changes carry
   their scope.
9. All times are stored as UTC instants and displayed in the event time zone.
