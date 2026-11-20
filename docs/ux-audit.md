# RF Ops interface audit (current build, Phase 4)

Audited with the real HF 2026 Saturday afternoon data at four sizes: wall 1920×1080, FOH laptop
1440×900, iPad portrait 820×1180, phone 390×844.

States covered: pre-show, show mode mid-handoff-song with a critical alert, performer sheet,
raise-alert form, every Admin tab, login and report.

Findings are tagged **BUG** (wrong behaviour), **RISK** (likely operator error during a show) or
**FRICTION** (slow or confusing).

## Cross-cutting

1. **BUG — replayed history fires notifications.** A screen that loads fresh connects with
   `after=0`, the server replays old events, and every past critical alert pops a red toast and
   vibrates again. Every reload or new device "re-alarms" (seen on login, admin and FOH captures).
2. **BUG — alerts don't follow the pack.** "IEM no audio" raised on Helen while Shosh holds RX10
   shows on Helen's (off-stage) card but **not** on the RX10 row at FOH or on Shosh's card. Alerts
   are tied to a person, but problems live in hardware.
3. **RISK — layout jumps between songs.** Cards re-sort by on/off stage, so positions change at
   every song change. Muscle memory and "the third card is Judah" break mid-show.
4. **RISK — alarm flood in show mode.** Every pack raises "Battery not checked", so every card gets
   ⚠1 and an amber outline, and FOH's IEM table turns entirely amber with a duplicated status
   column. The one real critical (Helen) is visually lost.
5. **FRICTION — "Unchecked" everywhere pre-show.** An amber battery badge on all 24 kits before
   call time is noise. "Not due yet" and "Due now" aren't distinguished.
6. **RISK — toasts cover content.** The centre-bottom toast sits over cards, the FOH table and Admin
   rows for 6 s. The critical toast duplicates the alert panel.
7. **FRICTION — truncation of the most important text.** The show name ("HF Afternoon — Saturd"),
   the song title on phone ("I Will Follow J…") and mix names ("Andrew Tayl…") overflow; Seth's
   battery badge still overflows its card.
8. **FRICTION — ambiguous kit naming.** Kits read "RX10 · ME 3", "ME 1 (shared)", "Console mix".
   On cards the *mix* is the bold label and the *pack* (what's physically handed over) is small
   grey text underneath.
9. **FRICTION — no "what do I do next" view.** Each A2 has to read all cards plus the handoff list
   to work out their own next actions. "Mine" is empty because no performer has an A2 assigned,
   and there's no way to assign them in bulk.
10. **FRICTION — checkout and handoffs are separate concepts.** The pre-show "GIVE" list *is* the
    checkout and the post-show "COLLECT" list *is* the return, but ticking them doesn't record a
    checkout, and the separate checkout feature has no UI at all.

## Backstage wall (1920×1080)

- Single-letter avatars waste space and carry no information. Use photos or initials only when no
  photo exists.
- Card body text is ~15–16 px; it needs about 24 px+ to read at 2 m (see research).
- The handoff list in pre-show is long (12 GIVE rows). It is fine as a checklist but pushes
  in-show info down. The "After show" COLLECT list appears while the last song is still running.
- Raise alert sits in the middle of the filter row; consistent placement matters.
- In the performer sheet: grammar ("1 pack need"); the header is small; there's no "who has this
  pack next"; the four battery buttons are equal weight, though "Checked OK" is the 90% case.

## Phone (390×844)

- The header takes 3 rows; the song title truncates.
- Order: song → alerts → full handoff list (12+ rows) → cards. The performer cards are ~2,000 px
  down. There's no tab or segmented navigation.
- The raise-alert modal opens in the middle of a scrolled page. Performer and equipment are long
  native selects.
- Toasts cover the handoff list.

## FOH (1440×900)

- There's no quick "mic down on Red" action. FOH must Raise → pick severity → preset → performer
  select → equipment select (5+ taps). FOH thinks in **channels/colours**: tapping a row should
  raise against that kit and its current performer.
- The battery column plus a status column duplicate "Battery not checked" on every IEM row.
- The IEM table sorts by mix text ("Andrew Taylor's mix…" first); FOH would expect pack/TX order.
- No sign of the RX10 problem (see bug 2).

## Admin

- **Assignments grid is performer-rows × songs, with "only performers with mic kits" on by
  default.** With colour mics unassigned the grid is *empty*, and the main task ("who's on
  Yellow?") requires unticking a box. Crews think in **mic plots: mics as rows, songs as
  columns, names in cells.**
- Kits can't be edited (delete only). Console channel, name, mix and spare flag can't be changed
  after creation.
- No bulk A2 ownership: you assign one performer at a time in People; zone is free text.
- Inventory has no inline label/serial edit and no "where is this item now" (who holds it).
- Show & kits: there's no setlist paste or import; songs are added one by one.
- The call-time date picker is empty for the template shows, so battery rules use "4 h since last
  check" only.

## Login / PIN / report

- Login is fine. Operator buttons could show which screen role they suit (wall, phone, FOH).
- Report: OK. It should also list *open* faults and unresolved alerts at the top, as the
  "pull for repair" list.
