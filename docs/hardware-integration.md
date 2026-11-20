# Hardware integration reference (verified Nov 2026)

These facts fix the data model before Phase 5.
- **V** = verified in a primary document (quoted or cited).
- **I** = inferred.
- **U** = unknown; needs a bench test (§8).

Source extracts are in the session scratchpad. The citations below point to the originals.

## 1. Shure ULX-D (ULXD4Q, TCP 2202)

Sources:
- Shure "ULX-D Command Strings" v1.0 (2022), mirror:
  goknight.com/content/documentation/ULXD_command-strings_en-US (1).pdf
- Shure Tech Bulletin 2018: content-files.shure.com/Pubs/ulx/ulx-d-network-string-commands.pdf
- ULXD4Q user guide (2022).

### Protocol basics

- **Framing (V):** ASCII frames: `< GET x P >`, `< SET x P v >`, `< REP x P v >`,
  `< SAMPLE x ALL nn aaa eee >`.
  - x = 0 means all channels; x = 1–4 is one channel.
  - Strings come in `{…}`, space-padded to a fixed width. CHAN_NAME is always 8 characters.
- **Push (V):** "With the exception of the metered properties, the device sends a REPORT when a
  value changes." This includes changes made on the front panel and in WWB.
  - Metered properties never push on change: RF_ANTENNA, RX_RF_LVL, AUDIO_LVL. Use
    METER_RATE / SAMPLE for those.
  - NET_SETTINGS has no asynchronous REPs.
- **Snapshot (V):** `GET 0 ALL` returns every property once.
- **Metering (V):** off by default. Turn it on with `SET x METER_RATE 00100–99999` (ms); 0 turns
  it off. **Use 1000 ms (I).**
- **Liveness (I):** keep one persistent socket per receiver. Shure: "Do not close and reopen the
  socket repeatedly."
  - SAMPLE traffic is the heartbeat.
  - Send `GET x BATT_BARS` every 20 s.
  - The receiver is stale after 3× the meter interval with nothing received.

### Monitoring properties

Sentinels map to NULL plus a state value, never stored as numbers.

| Property | Values | Notes |
|---|---|---|
| BATT_BARS | 000–005, **255 unknown** | All battery types |
| BATT_RUN_TIME | minutes 0–65532; **65533 comm error, 65534 calculating, 65535 unknown/NA** | SB900 only; "valid only for units in use, in range, out of charger"; ±15 min accuracy |
| BATT_CHARGE / BATT_HEALTH | 0–100 %, 255 unknown | Rechargeable only |
| BATT_CYCLE | 0–65534, 65535 unknown | |
| BATT_TEMP_C | value − 40, 255 unknown | |
| BATT_TYPE | LION ALKA NIMH LITH WARN UNKN | AA type is **user-set on the TX**, not detected |
| RX_RF_LVL | dBm = value − 128 | 2018 doc: raw 0–115; 2022 doc: 0–255. **Parse leniently, bench-confirm** |
| AUDIO_LVL | dBFS = value − 50 | 2018: 0–50; 2022: 0–255 |
| RF_ANTENNA | AX XB AB XX | Squelch/diversity LEDs |
| RF_INT_DET | NONE / CRITICAL | Pushed on change |
| ENCRYPTION_WARNING | ON = mismatch | |
| TX_TYPE | QLXD1 QLXD2 ULXD1 ULXD2 ULXD6 ULXD8 UNKN | UNKN = no transmitter linked |
| TX_DEVICE_ID | 8 characters, blank = unknown | **ULXD6/ULXD8 only.** Blank on our ULXD1/2 (I, bench) |
| TX_MUTE_STATUS | ON OFF UNKN | Documented for ULXD6/8; ULXD1/2 is U |
| TX_RF_PWR | LOW NORMAL HIGH UNKN | |
| FW_VER / TX_FW_VER | Maj.Min.Pack.Build; a trailing `*` means the last update was incomplete | |
| AUDIO_GAIN / AUDIO_MUTE | gain = value − 18 dB | dLive can change these. Log as an external change |
| HIGH_DENSITY | ON/OFF | HD: 1 mW, lower RF levels, letter group/channel |

### Consequences for RF Ops

- **No automatic "wrong transmitter on this channel" check.** TX_DEVICE_ID isn't reported by
  ULXD1/2. The available automatic signals are:
  - TX_TYPE: handheld vs bodypack mismatch.
  - TX_TYPE = UNKN: no transmitter linked.

  Transmitter identity stays **manual**. Handoffs record "gave Red", and the label is the identity.
- **Battery:**
  - SB900 packs give bars plus minutes.
  - AA gives bars only, and the type is whatever the TX menu says.
  - Store `battery_kind` per device so the UI knows what to expect.
- **Controllers:** Shure says "Use only one controller per system." RF Ops must be the only
  2202 client. No Companion, Crestron or Q-SYS on these receivers.
  - WWB and dLive most likely use Shure's SLP/SDT control (ports 8427/5568), not 2202 (I).
  - Coexistence is bench test #2–4.
- **Firmware:** receivers and transmitters must match MAJOR.MINOR. The latest are RX 2.9.0 and
  TX 2.10.0 (2025). Record FW_VER per receiver and flag a mismatch.

## 2. Allen & Heath dLive Shure integration

Sources: dLive Shure Integration Guide issue 1, and release notes V1.99–V2.11.

- **What it does (V):** monitors battery, RF and audio; controls gain and mute; auto-discovers up to
  45 receivers. Firmware ≥ 1.70, M-DANTE ≥ 4.0.3.8. Uses a network port on the Surface or MixRack;
  SPLIT or SWITCHED network modes; same subnet.
- **Transport (I):** most likely Shure SLP/SDT (the same path as WWB), not 2202. So dLive doesn't
  take a 2202 connection, but its gain and mute changes will reach RF Ops as unsolicited REPs.
  RF Ops logs them as **external changes**, not errors.
- **Traffic (V):** "When no Shure devices are connected it is recommended to Disable Shure
  Detection." Disabling needs a MixRack power cycle.

## 3. Sennheiser IEM

### ew IEM G4 SR (3 units)

Source: TI 1254 v1.0 (sennheiser.com …ti_1254_metromediensteuerung_ewg4_en.pdf).

- **Transport and format (V):**
  - UDP port 53212, the same port for send and receive.
  - Firmware ≥ 1.7.0.
  - ASCII `Command p1 … pN<CR>`, maximum 1500 characters.
  - Error codes 1000–1050.
- **Push (V):** `Push <timeout 0–300 s> <cycle ms> <mode 0–7>`. **It must be re-sent before the
  timeout**, or updates stop. Use `Push 250 1000 3` re-sent every 200 s (I).
- **Readable (V):**
  - Name, Frequency, Sensitivity, Mode (mono/stereo), Equalizer.
  - **Mute = RF carrier off**.
  - AF peak, States (RF mute), Msg (AF_Peak / RF_Mute / OK).
  - Config change counter, FirmwareRevision.
  - The `Af` field count is inconsistent in the doc, so parse tolerantly.
- **Not readable:** pack battery, whether a pack is on, pack tuning. RF to the pack is one-way.
- **WSM coexistence (V):** "simultaneously … possible … the last received command … will be
  adopted." WSM uses UDP 8133 + mDNS. RF Ops never sends SET to these, so it reads only.

### XSW IEM (10 units)

No network port (V, manual 03/2026). **Fully manual.**

## 4. Shure GLX-D / GLX-D+ (GLXD6, GLXD6+)

- **No network (V):** USB for firmware only. **Fully manual.**
- **Battery:** shown on the pedal (7 LEDs, ≥ 30 min … ≥ 11.5 h). TX LED red below 1 h, flashing
  below 30 min. Battery is SB902/SB902A.
- **Bands:** GLXD6+ Z3 uses 2400–2483.5 MHz **and 5725–5850 MHz**. A2 Wi-Fi must use 5 GHz
  UNII-1/2 only (channels 36–64), never 149–165, and never 2.4 GHz.

## 5. MultiTracks Playback MIDI

Sources: help centre "Playback user guide", "How to add/edit MIDI cues", Playback 8.5.4 release
notes (13 Apr 2026), multitracks.com MIDI Out product page.

- **Cues (V):** timeline events attached to song sections; types Note / CC / PC; 8 buses, each
  with its own output port and channel. **They fire when the playhead crosses them while playing.
  Nothing is sent on song select (I/U).**
- **Virtual source (V):** Playback 8.5.4 can act as a virtual MIDI source, so the RF Ops bridge
  can listen to Playback directly. The IAC bus and the PlayAUDIO1U DIN/RTP ports are
  alternatives.
- **Subscription (V, sources conflict):** the product page says Cloud Pro is required to send MIDI
  cues. The user guide says Premium can add cues locally. **Confirm on the account.**
- **Design consequences:**
  - The cue for song N must sit at **0:00 of song N**, so RF Ops moves when the song *starts*.
  - "Next song" displays come from RF Ops' own setlist.
  - A skipped song in Playback = a jump in RF Ops (see interface spec §9.4).

## 6. iConnectivity PlayAUDIO1U

DIN in/out 1×1, USB host (8 devices), Ethernet RTP-MIDI, and two USB-C host ports (V).
Class-compliant on macOS (I). It is only a fallback route.

## 7. What RF Ops can know automatically

| Device | Automatic (Phase 5) | Always manual |
|---|---|---|
| ULX-D channel + ULXD1/2 | Linked/no TX, TX type, battery bars, runtime/charge/health (SB900), RF dBm, antenna, audio dBFS, interference, encryption mismatch, TX RF power, frequency, channel name, gain/mute, firmware | Which physical TX is on the channel; capsule/element; AA freshness; TX mute (U) |
| SR IEM G4 (3) | Reachable, name, frequency, RF on/off, input peak, mono/stereo, firmware | Pack battery, pack on/tuned, earpieces |
| XSW IEM (10) | — | Everything |
| GLXD6/6+ (3) | — | Everything |
| Playback | PC/CC/Note at play | What's cued but not playing |

Every device record carries `telemetry_capability`, so the UI shows "manual" rather than
"no data" for things that can never report.

## 8. Bench tests (before Phase 5 goes live)

1. **TX_DEVICE_ID / TX_MUTE_STATUS on ULXD1 and ULXD2.** Send `GET 0 ALL`, toggle mute mode,
   capture REPs.
2. **Concurrent 2202 sessions.** Open 1, 2, 3, 5 and 6 nc sessions. Change the channel name on the
   front panel; all sessions should get the REP. Find the limit.
3. **METER_RATE scope.** Is it per connection? Session A at 1000 ms, session B at 0.
4. **WWB + dLive + RF Ops for 2 h.**
   - Wireshark: `tcp.port==2202`, `udp.port in {5568,8427}`.
   - Confirm dLive sends no 2202 traffic.
   - A dLive gain change should reach RF Ops as a REP.
   - WWB's device list should stay stable.
5. **Battery semantics.** For SB900 vs alkaline/NiMH/lithium AA, record BATT_TYPE, BARS,
   RUN_TIME and CHARGE at power-on, while calculating (65534), mid-show, and after power-off.
   Compare RUN_TIME against actual over 2 h.
6. **Meter scales.** Raw `aaa`/`eee` at 1 m, at 30 m and with the TX off, in normal and HD mode.
7. **Liveness.** Pull the receiver Ethernet for 10 s and for 90 s, and reboot it. Measure
   detection time and check half-open sockets are cleaned up.
8. **G4 SR + WSM.** Send `Push 60 1000 3` with WSM running. Both receive updates; a WSM rename
   arrives as `Name`; the push expiring stops only RF Ops. Check the `Af` field count and
   firmware ≥ 1.7.0.
9. **Playback.**
   - Put a PC cue at 0:00 on a dedicated bus, channel 16.
   - Test each action: select (expect nothing), play, stop/replay, jump to a section, Live
     ReOrder, and loop on/off.
   - Test each route: virtual source, IAC, and PlayAUDIO1U.
   - Confirm the subscription tier.
