# RF Ops data model (v3, authoritative)

This document is the single source of truth for what RF Ops stores and how holders, handoffs and
usage are worked out. It resolves review blockers 1–3, 4–5, 11, 16–17 and decisions 19–23, 30–33
(see `docs/interface-spec.md` §23).

## 1. Core idea in one paragraph

**Devices** (Red, Bpack2, RX10, GLX 1) are permanent and shared by every show. Each show has a
**plot**: a grid of device × song, with a person in each cell. Separately, RF Ops always knows
**who physically has each device right now** (the *holding*).

**A handoff is any difference between "who should have it" and "who has it".** Doing the handoff
updates the holding. Nothing else is ticked or stored, so reorders, MIDI jumps, going back,
changeovers and count-in all fall out of the same comparison. There are no tick keys to go stale.

## 2. Tables

### Physical inventory (unchanged in spirit)

**`equipment`:** one row per physical item.
- What it is: receivers, receiver channels, transmitters, capsules, headsets, IEM TX channels,
  IEM packs, guitar receivers/packs, rechargeable batteries.
- Adds:
  - `battery_kind` (aa_alkaline | aa_nimh | aa_lithium | sb900 | sb902 | none)
  - `owner` (NULL = house; text = guest owner)
  - `telemetry` (set: battery_bars | runtime | rf | tx_link | none)
  - `status`: available | suspect | out_of_service | missing
- `label` stays unique, so guest items get a suffix ("Chandler's pack (Sat)").

### Devices (new; the board's tiles)

**`devices`**, global:
- `id`, `label` (Red), `kind` (mic | iem | guitar)
- `carrier_id` → equipment (transmitter / IEM pack / guitar pack). Unique among active devices.
- `base_id` → equipment:
  - mic: receiver channel. Unique among active mic devices.
  - iem: TX channel. **May be shared.**
  - guitar: receiver. Unique.
- `element_id` → default capsule / headset / lav (nullable)
- `console_channel`, `console_name`, `default_mix` (IEM)
- `section` (mics | iem | guitar | spare | guest), `board_order`
- `zone_override` (nullable)
- `is_spare`, `active`
- `guest_show_id` (nullable: guest devices exist only for one show)
- `version` (optimistic concurrency)

**`show_devices`:** per-show overrides.
- `show_id`, `device_id`
- `mix` (the ME-1 scene changes per set), `tbc` (JSON flags), `excluded` (not used in this show)

### Events, shows, songs

**`events`:**
- `id`, `name` ("Homestead Fair 2026")
- **`timezone` (IANA, e.g. America/Chicago)**
- `starts_on`, `ends_on`

**`shows`:**
- `id`, `event_id`, `name`, **`day` (local date)**, `set_name` (afternoon | evening | …)
- `concert_at` and `call_at`: UTC instants, entered and shown in the event's time zone. Call
  defaults to concert − 1 h.
- `mode`, `midi_mode`, `current_song_id`
- **`is_active`:** a partial unique index allows only one active row.
- **`practice_of`** (nullable: practice copy of a show)
- `linked_prev_show_id` (the afternoon show for an evening; Friday for Saturday)

**`songs`:** as now.
- PC number bound to the song.
- `skipped` (a persistent Lead decision).
- A jump never sets `skipped`.

### The plot

**`plot_cells`:**
- Primary key `(show_id, device_id, song_id)`. `song_id` NULL = the **Default** column.
- `performer_id`: NULL = **nobody**.
- `element_id`: NULL = the device's default element. Lets a cell say "Red with KSM9" for a
  capsule swap.

**Lookup rules:**
- *Planned holder* of device D in song S = the cell (S), else the cell (Default), else nobody.
- A missing Default cell means nobody.

### Passes (a person-to-person change during a song)

**`passes`:**
- `id`, `show_id`, `device_id`, `song_id`, `from_performer`, `to_performer`
- `state`: planned | done | cancelled
- `done_by`, `done_at`

**How a pass behaves:**
- Planned holder at the *start* of the song is `from` (the plot cell). Once the pass is done it is
  `to`.
- The **return** is automatic. At the next boundary the plot for the next song says Helen, the
  holding says Shosh, so a pass-back handoff appears with no special "and back" flag.
- **Example:** the seeded Helen → Shosh RX10 override pair becomes one pass on RX10 during
  *I Will Follow Jesus*, plus plot Default = Helen.

### Substitutions (swaps, pulls, guest replacements)

**`substitutions`:**
- `id`, `show_id`
- `failed_device_id`
- `replacement_device_id` (NULL = **pulled**, no replacement)
- `performer_id` (NULL = every holder of the failed device)
- `from_song_id`, `to_song_id` (NULL = open-ended)
- `kind`: swap | pull | guest
- `scope`: this_song | until_fixed | rest_of_show
- `state`: recorded | in_hand | foh_confirmed | reversed | returned
- `failed_component` (unknown | carrier | element | channel)
- Who/when for each state change.

**Overlay on planned holder:** for songs in [from, to] where the failed device's planned holder
matches `performer_id` (or any holder when NULL):
- the replacement's planned holder becomes that performer;
- the failed device's planned holder becomes nobody.

| Scope | from | to | performer | Notes |
|---|---|---|---|---|
| **this_song** | current | current | the holder | |
| **until_fixed** | current | open | the holder | Closed by **Return** (to = the song before the return) |
| **rest_of_show** ("Replace Red with Tan for the rest of the show") | current | open | NULL | Moves every later Red cell to Tan |
| **pull** | current | per scope | per scope | Replacement NULL. The performer has no device of that kind; the tile shows "Pulled" |
| **guest** | current, or Default if pre-show | open | NULL | Replacement = the guest device |

If a replacement then collides with its own later plot (Tan is planned for X at Song 5), that is a
**conflict** (§5). One song ahead it raises: "Tan needed for X at Song 5; Red still out"
[Find another] [Give X a different mic] [Tell FOH].

### Holdings (physical truth)

**`holdings`:** one current row per device per **scope**.
- Scope = real, or a practice show ID.
- Fields: `holder_performer_id` (NULL = in stock), `element_id` (capsule/headset fitted now),
  `since`, `by`, `via` (give | collect | pass | swap | count_in | manual | carry_over).
- History lives in `event_log` (`holding.changed`).
- Holdings are global. They carry over between shows on their own: afternoon → evening, Friday →
  Saturday.

### Alerts

**`alerts`** add:
- `type` (the interface spec §7.6 table)
- `device_id`, or `equipment_id` for a TX/receiver-level alert. Device or equipment is required
  for device problems.
- `holder_at_raise`, `song_at_raise`, `failed_component`
- `repeat_count`, `snoozed_until_song_id`, `owner_operator_id`, `on_it_by`, `on_it_at`
- `scope` (real | practice show)

### Battery checks

**`battery_checks`** add: `device_id`, `battery_kind`, `line_check` (bool), `scope`.

**Evidence rule (blocker 17).** A check counts as evidence **for show S** only if it was recorded
at or after `S.call_at − 2 h`.

| Check recorded | Shown in show S |
|---|---|
| Earlier, same day | amber "checked for earlier show (13:02)" |
| Earlier, another day | "✓ Fri 19:02", amber after call |

All times are displayed in the event time zone.

### Operators, sessions, screens

**`users`:** PIN mandatory (`pin_hash` NOT NULL) except role `helper`.

**`screens`** (named, enrolled device credentials):
- `id`, `name` (Wall, Wall SR, FOH), `role_view`, `secret_hash`
- `enrolled_by`, `enrolled_at`, `last_seen_at`, `expires_at` (7 days, sliding; never expires
  while any show is in show mode), `revoked`

**`session_revocations`:**
- Sessions are **signed, stateless cookies**: HMAC with `RFOPS_SESSION_KEY` over
  (sid, user_id | screen_id, client_id, issued_at, expires_at). That way they survive a standby
  takeover.
- The table holds revoked `sid`s until they expire. `users.sessions_valid_after` revokes every
  session for that user at once.
- 18 h sliding (re-issued on use after 1 h), never expiring while in show mode.

**`join_codes`:**
- `code_hash`, `user_id` (named helper), `uses_left` (3), `expires_at` (2 h), `created_by`

**`push_subscriptions`:** `user_id` / `screen_id`, endpoint, keys.

**Attribution.** Wall actions are attributed "Wall", or "Wall (Sam)" when someone tapped their
name chip in the last 5 min.

### Other tables

- `zones` (managed list)
- `performers.zone_id`
- `performer_absences(show_id, performer_id)`
- `checklists` / `checklist_items` / `checklist_ticks`
- `messages` (phrase, device, from, to, urgent, acked)
- `settings` (epoch, keys)

## 3. Derived values (pure functions in `rfops-core`)

1. **`planned(show, device, song)`:**
   - cell(song) → cell(default) → nobody;
   - then apply substitutions, in creation order;
   - then the pass state: during song S, a done pass sets the holder to `to`.
2. **`physical(device)`:** the holdings row in the current scope.
3. **`targets(show)`:** the comparison points:
   - **Now:** the current song.
   - **Next:** the next non-skipped song after the current one, or "after show".
   - **During-song passes** in the current song.
4. **`handoffs(show, target)`:** for each device in the show, compare `physical(device)` with
   `planned(device, target)`:
   - stock → P: **give**
   - P → stock (target says nobody, including after show): **collect**
   - P1 → P2: **pass**
   - same holder, different element: **capsule swap**

   Handoff identity (UI only, recomputed every time): (device, type, to-holder, target). Doing it
   writes a holding change; nothing else is stored.
5. **Overdue:** handoffs against **Now** (the current song) are overdue the moment the song is
   current. They get a WARNING on the device; actions are **Done now** and **Leave it** (a
   recorded exception for this song).
6. **Coming up:** handoffs against **Next**, plus passes in the next song (shown one song ahead
   for prep).
7. **Jumps** (MIDI 2 → 4):
   - The current song becomes 4.
   - Now = planned(4) vs physical, which is exactly the combined change.
   - Song 3's handoffs simply never appear.
   - Passes in songs jumped over get `cancelled` with reason "jumped".
   - A pass in the departed song that wasn't done is overdue only if its device is still planned
     to change in the new song. Otherwise it is cancelled.
   - A jump never sets `skipped`; a later jump back works normally.
8. **Going back** (4 → 3): the same comparison. The "Reverse?" card is just the Now handoffs.
   Nothing is undone automatically.
9. **Usage** of device D in the active show at current song C:

   | Condition | Usage |
   |---|---|
   | planned(D, C) ≠ nobody | **in use** |
   | Nobody now, but planned in a later song | **next** |
   | Nobody now or later, physical = stock | **done** |
   | Nobody now or later, physical = someone | **collect from <holder>** |
   | `is_spare` and unplanned | **spare** |
   | Replacement in an active substitution | **on spare for <failed>** |
   | Failed device in an active pull | **pulled** |
   | Carrier `suspect` / `out_of_service` | **suspect / out** |
   | Carrier `missing` | **missing** |
   | `guest_show_id` set | **guest** |

   Precedence: out/missing > pulled > on spare > in use > collect > next > spare > done.
10. **Device zone:**
    - `zone_override`, else the zone of the device's first planned holder in this show, else
      none.
    - Used for battery-service filters and alert ownership.
11. **Conflicts** for a show and song:
    - **Error:** the same performer holds two IEM packs or two guitar packs.
    - **Error:** the same element is used by two devices.
    - **Error:** a replacement device is also planned for someone else.
    - **Warning:** a performer holds two mics (allowed, but flagged).
    - **Warning:** a planned holder is absent.
    - **Note:** a mic receiver channel or TX shared between active devices is prevented at device
      level, except IEM TX, which may be shared.

## 4. Practice shows (blocker 11)

**Creating one:** "Practice show" copies the show (plot, passes) with `practice_of` set.

**Isolation.** Every write made while viewing a practice show is stored with
`scope = practice show ID`:
- holdings (a separate set, starting from stock);
- battery checks;
- alerts;
- substitutions.

Equipment status changes made in practice are recorded only in a per-scope override, never on
`equipment`.

**Never affects the real show:**
- A practice show can never become active.
- MIDI is ignored unless the practice show has "Practice with Playback" on.
- Rule alerts for it only appear on screens pinned to it.
- Practice data is excluded from the report, evidence and pinned backups.

**Look:** "PRACTICE" watermark on every screen.

## 5. Migration from the current schema (0001–0004)

Run as migration `0005_v3_model.sql` plus a Rust data step, both inside one transaction, with a
verified backup taken first.

1. **Events.** Create one event "Default event" (time zone from `RFOPS_TZ`, default
   America/Chicago) and attach every show.
2. **Show fields.**
   - `starts_at` → `concert_at`.
   - `call_time` → `call_at`.
   - `day` = local date of `concert_at`, or the creation date.
   - The show in `mode='show'`, else the highest id, becomes active.
3. **Devices.** For each distinct (kind, carrier_id, base_id) across `kits`, create one device:
   - `label` = the carrier's equipment label (Yellow, RX10…);
   - `console_channel` / `console_name` / `default_mix` from the first kit;
   - `element_id` from the most common element;
   - `board_order` = section order, then console channel or RX number.

   A kit whose console channel or mix differs from the device's goes into `show_devices.mix`.
   Kits with no carrier keep their base only (`kind` from the kit).
4. **Plot cells.** For each show:
   - Default column: from each performer's default assignments, mapped kit → device.
   - For every song, compute the old effective assignment with the old `effective_assignment` and
     write a cell only where it differs from Default.
   - Kits that differ only by element (e.g. "Lead Vox" vs "Lead Vox KSM9", same TX) → the cell's
     `element_id`.
5. **Passes.** A song override pair that moves one device from A to B *within the same song*
   (A off, B on, same device) → a pass in that song. Default = A. This is exactly the
   Helen/Shosh pattern.
6. **Holdings.** For each device:
   - Start from stock.
   - Apply the open `checkouts` (equipment → device) and the latest `handoff_done` rows of the
     active show, in order: issue→give, return→collect, transfer→pass, component_change→capsule.
   - The −1 sentinels mean before/after show.
   - Then drop `handoff_done`.
7. **Swaps → substitutions:**
   - scope `song` → this_song, `rest_of_show` → until_fixed.
   - Historical rows get state `foh_confirmed`.
   - Kit IDs map to device IDs.
8. **Alerts:**
   - severity → priority.
   - `type` from the dedupe_key prefix: `battery:` → battery_not_checked, `midi-unmatched:` →
     midi_no_song, `midi-bridge:` → bridge_offline. Everything else becomes `other`.
   - State: open → NEW; acknowledged → ON IT; resolved → FIXED, or `cleared` if
     `resolved_by='system'`.
   - `performer_id` → `holder_at_raise`.
9. **People and checks.**
   - Distinct non-empty `performers.zone` strings become `zones` rows.
   - `battery_checks.device_id` comes from carrier → device.
10. **Operators and old tables.**
    - Every user without a PIN is flagged `must_set_pin`, and writes are refused until the Lead
      sets PINs (first-login flow).
    - The bootstrap "any name acts" mode is removed. The first admin's PIN comes from
      `RFOPS_ADMIN_PIN` on first start.
    - The old `kits` and `assignments` tables are kept read-only for one release, then dropped.

**Moving data to Render:** take a verified local backup, then Admin → Data → **Restore from
file** (Lead + PIN) on the new service. That runs the same migrations.

## 6. Invariants (enforced and tested)

1. At most one active show.
2. A device's carrier is unique among active devices. A mic channel / guitar receiver is unique
   among active devices of that kind.
3. A plot cell refers to a device and song of the same show, and to an existing performer.
4. A holding refers to an active device. A guest device's holding must be stock or its owner
   before its show is archived.
5. A substitution's replacement is of the same kind. A handheld ↔ bodypack replacement is allowed
   and flagged.
6. Every state change writes an `event_log` row in the same transaction. Practice changes carry
   the scope.
7. All times are stored as UTC instants and displayed in the event time zone.
