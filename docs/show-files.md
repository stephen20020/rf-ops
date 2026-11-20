# Building a show from a file

Every show can be described in one plain text file (YAML) and loaded into RF Ops with one command.
You can write the file by hand, copy last year's and edit it, or ask an AI agent (for example
Claude Code) to write it from the band's PDFs, spreadsheets or emails.

```bash
rfops show check  my-show.yaml           # plain-English problems; changes nothing
rfops show import my-show.yaml           # create the show, or update the one with that name
rfops show export "HF Afternoon — Friday" -o friday.yaml   # any show back out as a file
```

Set the server once: `export RFOPS_SERVER=https://rfops.<your domain>` (local:
`http://localhost:8080`). `RFOPS_NAME="Your name"` puts your name on the change in the log.

## Asking an agent to build a show

Give the agent the documents and this instruction:

> Read `docs/show-files.md`. Write a show file for <show> from these documents. Use only device
> labels that RF Ops already has (run `rfops show export` on an existing show, or `curl
> $RFOPS_SERVER/api/devices`, to see them). Then run `rfops show check` and fix every error
> until it reports none. List anything the documents didn't say (TBC) as a comment in the
> file. Don't import until I've read it.

`check` names each problem and suggests the closest correct name ("Juda isn't in people. Did you
mean "Judah"?"). An agent can loop on it until the file is clean.

## The format

```yaml
show: HF Afternoon — Friday        # name; importing again updates this show
event: Homestead Fair 2026         # optional; groups shows (and sets the time zone)
timezone: America/Chicago          # optional (IANA name)
day: 2026-06-19                    # optional, YYYY-MM-DD
set: afternoon                     # optional label
concert: "14:00"                   # optional, 24 h
call: "13:00"                      # optional; defaults to one hour before the concert
follows: HF Afternoon — Thursday   # optional: same-day show this one carries devices over from

people:                            # everyone named in the plot or passes
  - Helen
  - { name: Shosh, zone: SR, notes: "vocal" }
  - { name: Chandler Moore, absent: true }   # not here for this show

devices:                           # optional: only devices RF Ops doesn't have yet
  - { label: Chandler's pack, kind: iem, base: TX9, owner: Chandler Moore }
  - { label: Spare HH, kind: mic, base: ULXD-3/4, channel: "44", spare: true }

mixes:                             # IEM transmitter → mix heard on it in this show
  TX1: ME 1
  TX10: ME 3

songs:                             # in running order
  - { title: Walk-in, pc: 1 }      # pc = the MIDI Program Change Playback sends
  - title: I Will Follow Jesus / Lace Up Your Boots
    pc: 5
    passes:                        # hand-overs DURING the song, in the order they happen
      - "RX10: Helen -> Shosh"
  - { title: Encore, pc: 9, skip: true }   # in the list, but not planned to play

plot:                              # who has each device
  default:                         # unless a song says otherwise
    Red: Helen
    RX10: Helen
  Walk-in:                         # only the differences for that song
    Red: nobody
```

**Rules** (all checked by `rfops show check`):
- Names must match exactly between `people`, `plot` and `passes`. Song titles in `plot` must match
  `songs`.
- Each song title appears once. Each `pc` number is used once.
- **Passes:** the *from* person must be whoever has the device at that point. The return after
  the song needs no pass: if the plot says Helen and Shosh has it, RF Ops asks for it back at the
  next song.
- **Errors:** nobody may have two IEM packs (or two guitar packs) at once. Two mics is only a
  warning.
- `nobody`, `none` or `-` means the device isn't used.
- Kinds are `mic`, `iem` or `guitar`. `base` is the transmitter (IEM) or receiver channel (mic or
  guitar), by its inventory label.
- A file can list every device it uses. Devices RF Ops already has stay as they are. New ones are
  added, and so is any transmitter or receiver channel named in `base` that isn't in the inventory
  yet (shown as a warning). So one file works on any RF Ops database.

## Updating a show that's already running

Importing again is safe during a show:
- Songs are matched by title, so their history (what was played, passes done) stays.
- A song that was already played can't be removed; mark it `skip: true` instead.
- Each change is logged with who made it.
