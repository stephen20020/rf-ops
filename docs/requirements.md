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
| Playback | MultiTracks Playback on a **MacBook** (macOS version TBD), **iConnectivity PlayAUDIO1U** | MIDI via RF Ops bridge on the Mac (Phase 4) |
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

Template: `rfops-server --seed-hf2026` on an empty database loads this layout. Performer names for
the colour mics are left blank to fill in.

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
