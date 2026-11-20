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

## Operations

- Many performers and many handoffs per show.
- Multiple A2s working at the same time; FOH engineer also uses the system.
- Wall-mounted touchscreen backstage, plus phones/tablets for roaming A2s, plus FOH laptop/tablet.

## Design targets

60 performers, 12 receivers (48 ch), 20 IEM channels, 40 songs/segments, 200 handoffs per show, 10 concurrent clients.

## Open questions

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
