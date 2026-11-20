# Requirements & site facts

Living document. Confirmed facts come from the owner; everything else is marked open.

## Confirmed equipment

| Category | Equipment | Network telemetry |
|---|---|---|
| Mic receivers | Shure ULX-D **ULXD4Q** (4 ch each) — count TBD | Yes — Shure command strings, TCP 2202 (Phase 5) |
| Mic transmitters | ULX-D handhelds w/ interchangeable capsules; bodypacks w/ headsets/lavs | Via receiver (battery, RF, TX type/ID) |
| IEM | 10× **Sennheiser XSW IEM** (SR + EK) | None — manual only |
| IEM | 3× **Sennheiser SR IEM G4** (300 series), Ethernet confirmed | Transmitter status via Sennheiser ew G4 Media Control Protocol, UDP 53212 (Phase 5b). EK pack battery never available. |
| Guitar wireless | 2× **Shure GLXD6 (Z2, 2.4 GHz)**, 1× **Shure GLXD6+ (Z3, dual band)** | None — manual only |
| Console | Allen & Heath **dLive DM64 + S7000** | Optional read-only link via dLive MIDI-over-TCP (Phase 7) |
| Playback | MultiTracks Playback on a **MacBook** (macOS version TBD), **iConnectivity PlayAUDIO1U** | `rfops-bridge` on the Mac: virtual "RF Ops" MIDI port → server. Default cues: Program Change per song on ch 16, follow automatically. **Needs a real test with Playback.** |
| Network | Dante and control on the **same network** | — |

## Console system (from the HF 2026 dLive system plan, Nov 2026)

- Rig: S7000 surface, DM64 MixRack, 4× DX168 (DX 1/2 band station, DX 3/4 orchestra pit).
- **Wireless mics: 12 channels on console Ch 33–44, DM64 inputs 53–64.** Vocal mic IDs are
  colours: Yellow, Red, Blue, Green, Black, White, Orange, Tan (handhelds), then Bpack1–4. They are
  fader bank 1 on Layer A in that order. Direct outs to ME channels 23–34.
- **IEM: transmitters stay fixed all day; only the people change.**
  - TX1 ← ME-1 1 (drums), TX2 ← ME-1 2 (bass), TX4 ← ME-1 4 (Nord), TX10 ← ME-1 10 (acoustic).
  - TX7–TX9 ← St Aux 7–9 (Choir Director, Orchestra Director, Chandler Moore), on DM64 outs 13–18.
  - TX3, TX5 and TX6 are unused.
  - **RX1, RX3 and RX6 all listen to TX1**; every other pack matches its TX number.
  - One IEM transmitter feeding several packs is normal, so RF Ops does not treat it as a conflict.
    A pack or a mic receiver channel on two people still is.
- ME-1 6 feeds Andrew's own IEM in the evening (not in RF Ops inventory).
- Playback on Ch 109–116 (PB Click, PB Guide, Tracks, BGV, Keys/Pad), moving to Dante. PB Guide
  plus talkback go to ME-1 via the Comms aux.
- There are two sets (afternoon/evening) with a changeover. The run order isn't set yet.
- Open item in the plan: RF coordination for the 12 wireless channels.

## Afternoon IEM & ME-1 (Homestead Fair sheet, Fri/Sat, ME-1 scene 5)

| Person | Pack | Mix |
|---|---|---|
| Enoch (drums) | RX1 | ME 1 |
| Aaron (bass) | RX2 | ME 2 |
| Drummer 2 | RX3 | ME 1 (shared) |
| Tim (Nord) | RX4 | ME 4 |
| Judah (vocal) | RX5 | ME 5 |
| Drummer 3 | RX6 | ME 1 (shared) |
| Stephen (choir director) | RX7 | Console mix |
| Sam (orchestra director) | RX8 | ME 16 |
| Chandler Moore (Saturday) | RX9 | Console mix |
| Helen (vocal) | RX10 | ME 3 |
| David (acoustic/vocal) | Gabe's Shure | ME 10 |
| Seth (vocal) | Sennheiser 1 | Andrew Taylor's mix (ME unconfirmed) |
| Josh Patterson (Friday) | Sennheiser 2 | ME 15 (choir wedge) |

- Wired ME-1 users (Jacob perc ME 7, Blair EG2 ME 8, Joel EG3 ME 9, Matthew Garrett synth ME 11,
  Matthew Taylor B3 ME 12) are outside RF Ops. ME 13/14 are front-line/choir wedges; ME 6 is
  unassigned.
- **Handoff:** during "I Will Follow Jesus / Lace Up Your Boots", Shosh takes Helen's RX10. Exact
  timing and the return are unconfirmed.
- **Pre-concert:** check pack and mic batteries, RF and audio one hour before. In RF Ops, set the
  show's call time to concert start minus one hour; battery checks are then due from that time.

Template: `rfops-server --seed-hf2026` on an empty database loads "HF Afternoon — Friday" and
"HF Afternoon — Saturday" from this sheet. It includes the RX10 handoff, with placeholder
segments around the handoff song until the run order is set. The 12 colour mics are on Ch 33–44,
unassigned.

### Afternoon sheet vs. system plan (to resolve)

1. **TX5/RX5:** the plan says TX5 and ME-1 5 are unused; the sheet has Judah on RX5, ME 5.
2. **RX10:** the plan says TX10 ← ME-1 10 (acoustic); the sheet has Helen on RX10 with ME 3, and
   David (acoustic) on Gabe's Shure with ME 10.
3. **RX8:** the plan says TX8 ← St Aux 8 (orchestra director console mix); the sheet has Sam on
   ME 16.
4. **ME 6:** the plan says ME-1 6 feeds Andrew's own IEM in the evening; in the afternoon it is
   unassigned, and Seth is on "Andrew Taylor's mix (ME unconfirmed)".
5. Which transmitters feed Gabe's Shure, Sennheiser 1 and Sennheiser 2, and are any of these the
   3 SR IEM G4s?
6. Does Shosh hand RX10 back to Helen after the song?

## Operations

- Many performers and many handoffs per show.
- Multiple A2s working at the same time; FOH engineer also uses the system.
- Wall-mounted touchscreen backstage, plus phones/tablets for roaming A2s, plus FOH laptop/tablet.

## Design targets

60 performers, 12 receivers (48 ch), 20 IEM channels, 40 songs/segments, 200 handoffs per show, 10 concurrent clients.

## Open questions

- [ ] Which IEM transmitters/packs are XSW and which are SR IEM G4 (3 G4 have Ethernet).
- [ ] Who wears RX3 and RX6 (both on the TX1 drum mix)?
- [ ] TX7–TX9: the plan has them as director/Chandler mixes, and also as console mixes for
      Stephen, Sam and Chandler in the afternoon. Who wears which pack in each set?
- [ ] Capsules on the colour handhelds and headsets/lavs on Bpack1–4.
- [ ] Spare mics: all 12 ULX-D channels are in use. Is there a spare handheld or receiver channel?
- [ ] Which guitarists use the GLXD6/GLXD6+ (the plan wires EG1–3 to the band box and DM64 DIs)?

- [ ] Number of ULXD4Q receivers and their firmware versions.
- [ ] Is the dLive's built-in Shure integration in use? (Affects the concurrent-connection bench test.)
- [ ] macOS version on the Playback MacBook.
- [ ] How Playback's MIDI is set up per song (Program Change? channel?) and whether MIDI Out is enabled on the account.
- [ ] GLXD6+ second-band frequency range (for the Wi-Fi channel plan).
- [ ] Touchscreen model and size for the wall mount.
- [ ] Server host hardware (mini PC / Mac) and where it sits; UPS availability.
- [ ] How A2s divide work today (by performer? stage side? band vs vocals?).
- [ ] Current spare procedure and how many spare kits per show.
- [ ] Battery routine (AA vs rechargeable, when checks happen).
- [ ] Existing paperwork/spreadsheets to import (mic plot, handoff sheets).
- [ ] What FOH needs on screen at a glance.

## Risks to bench-test (Phase 5)

- ULXD4Q with dLive, Wireless Workbench and RF Ops connected at the same time.
- SR IEM G4 with Sennheiser WSM and RF Ops at the same time.
- Wi-Fi APs for A2 phones vs. GLX-D 2.4 GHz and Dante multicast on the shared network.
