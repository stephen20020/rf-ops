//! MIDI message parsing and song-trigger matching. Channels are 1–16 as musicians count them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MidiMsg {
    ProgramChange {
        channel: u8,
        value: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
}

impl MidiMsg {
    /// Parse a short MIDI message. Ignores everything RF Ops doesn't use (clock, active sensing,
    /// note-off, aftertouch, pitch bend, SysEx). Note-on with velocity 0 is a note-off.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let status = *bytes.first()?;
        let channel = (status & 0x0F) + 1;
        let d1 = bytes.get(1).copied().filter(|b| *b < 0x80);
        let d2 = bytes.get(2).copied().filter(|b| *b < 0x80);
        match status & 0xF0 {
            0xC0 => Some(MidiMsg::ProgramChange {
                channel,
                value: d1?,
            }),
            0xB0 => Some(MidiMsg::ControlChange {
                channel,
                controller: d1?,
                value: d2?,
            }),
            0x90 if d2? > 0 => Some(MidiMsg::NoteOn {
                channel,
                note: d1?,
                velocity: d2?,
            }),
            _ => None,
        }
    }

    pub fn to_bytes(self) -> Vec<u8> {
        match self {
            MidiMsg::ProgramChange { channel, value } => {
                vec![0xC0 | (channel.clamp(1, 16) - 1), value & 0x7F]
            }
            MidiMsg::ControlChange {
                channel,
                controller,
                value,
            } => {
                vec![
                    0xB0 | (channel.clamp(1, 16) - 1),
                    controller & 0x7F,
                    value & 0x7F,
                ]
            }
            MidiMsg::NoteOn {
                channel,
                note,
                velocity,
            } => {
                vec![
                    0x90 | (channel.clamp(1, 16) - 1),
                    note & 0x7F,
                    velocity & 0x7F,
                ]
            }
        }
    }

    pub fn describe(self) -> String {
        match self {
            MidiMsg::ProgramChange { channel, value } => {
                format!("Program Change {value} · ch {channel}")
            }
            MidiMsg::ControlChange {
                channel,
                controller,
                value,
            } => format!("CC {controller} = {value} · ch {channel}"),
            MidiMsg::NoteOn {
                channel,
                note,
                velocity,
            } => format!("Note {note} vel {velocity} · ch {channel}"),
        }
    }
}

/// What a song (or the next/previous action) listens for. Stored as JSON on the song, e.g.
/// `{"type":"program_change","channel":16,"value":5}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    ProgramChange {
        channel: u8,
        value: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    NoteOn {
        channel: u8,
        note: u8,
    },
}

impl Trigger {
    pub fn matches(&self, m: &MidiMsg) -> bool {
        match (*self, *m) {
            (
                Trigger::ProgramChange { channel, value },
                MidiMsg::ProgramChange {
                    channel: c,
                    value: v,
                },
            ) => channel == c && value == v,
            (
                Trigger::ControlChange {
                    channel,
                    controller,
                    value,
                },
                MidiMsg::ControlChange {
                    channel: c,
                    controller: cc,
                    value: v,
                },
            ) => channel == c && controller == cc && value == v,
            (
                Trigger::NoteOn { channel, note },
                MidiMsg::NoteOn {
                    channel: c,
                    note: n,
                    ..
                },
            ) => channel == c && note == n,
            _ => false,
        }
    }

    pub fn parse_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }

    pub fn channel(&self) -> u8 {
        match *self {
            Trigger::ProgramChange { channel, .. }
            | Trigger::ControlChange { channel, .. }
            | Trigger::NoteOn { channel, .. } => channel,
        }
    }
}

/// What the server should do with an incoming message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    GoToSong {
        song_id: i64,
    },
    Next,
    Prev,
    /// On a channel that songs listen to, but no song matches: worth a warning.
    Unmatched,
    /// Not on any channel RF Ops listens to.
    Ignore,
}

/// Decide what a message means for a show. `songs` are (song id, trigger) for non-skipped songs.
pub fn decide(
    m: &MidiMsg,
    songs: &[(i64, Trigger)],
    next: Option<Trigger>,
    prev: Option<Trigger>,
) -> Decision {
    if next.is_some_and(|t| t.matches(m)) {
        return Decision::Next;
    }
    if prev.is_some_and(|t| t.matches(m)) {
        return Decision::Prev;
    }
    if let Some((id, _)) = songs.iter().find(|(_, t)| t.matches(m)) {
        return Decision::GoToSong { song_id: *id };
    }
    let channel = match *m {
        MidiMsg::ProgramChange { channel, .. } => channel,
        // Only program changes are expected per song; stray CCs/notes are not warnings.
        _ => return Decision::Ignore,
    };
    if songs
        .iter()
        .any(|(_, t)| matches!(t, Trigger::ProgramChange { .. }) && t.channel() == channel)
    {
        Decision::Unmatched
    } else {
        Decision::Ignore
    }
}

/// Suppresses the same message repeated within a window (Playback may send a cue twice, or a
/// retried HTTP request may deliver it again).
#[derive(Debug, Default)]
pub struct Dedup {
    last: Option<(MidiMsg, u64)>,
}

impl Dedup {
    pub const WINDOW_MS: u64 = 500;

    /// Returns true if the message should be processed. `now_ms` is a monotonic timestamp.
    pub fn accept(&mut self, m: MidiMsg, now_ms: u64) -> bool {
        if let Some((prev, at)) = self.last {
            if prev == m && now_ms.saturating_sub(at) < Self::WINDOW_MS {
                return false;
            }
        }
        self.last = Some((m, now_ms));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(channel: u8, value: u8) -> MidiMsg {
        MidiMsg::ProgramChange { channel, value }
    }

    #[test]
    fn parse_and_roundtrip() {
        assert_eq!(MidiMsg::parse(&[0xCF, 5]), Some(pc(16, 5)));
        assert_eq!(MidiMsg::parse(&[0xC0, 0]), Some(pc(1, 0)));
        assert_eq!(
            MidiMsg::parse(&[0xB2, 7, 100]),
            Some(MidiMsg::ControlChange {
                channel: 3,
                controller: 7,
                value: 100
            })
        );
        assert_eq!(
            MidiMsg::parse(&[0x90, 60, 90]),
            Some(MidiMsg::NoteOn {
                channel: 1,
                note: 60,
                velocity: 90
            })
        );
        for m in [
            pc(16, 5),
            MidiMsg::ControlChange {
                channel: 3,
                controller: 7,
                value: 1,
            },
            MidiMsg::NoteOn {
                channel: 9,
                note: 36,
                velocity: 1,
            },
        ] {
            assert_eq!(MidiMsg::parse(&m.to_bytes()), Some(m));
        }
    }

    #[test]
    fn parse_ignores_noise() {
        assert_eq!(MidiMsg::parse(&[0xF8]), None, "clock");
        assert_eq!(MidiMsg::parse(&[0xFE]), None, "active sensing");
        assert_eq!(
            MidiMsg::parse(&[0x90, 60, 0]),
            None,
            "note-on vel 0 = note-off"
        );
        assert_eq!(MidiMsg::parse(&[0x80, 60, 0]), None, "note-off");
        assert_eq!(MidiMsg::parse(&[0xC0]), None, "truncated");
        assert_eq!(MidiMsg::parse(&[0xC0, 0x90]), None, "bad data byte");
        assert_eq!(MidiMsg::parse(&[]), None);
    }

    #[test]
    fn trigger_json() {
        let t = Trigger::parse_json(r#"{"type":"program_change","channel":16,"value":5}"#).unwrap();
        assert_eq!(
            t,
            Trigger::ProgramChange {
                channel: 16,
                value: 5
            }
        );
        assert!(t.matches(&pc(16, 5)));
        assert!(!t.matches(&pc(15, 5)));
        assert!(!t.matches(&pc(16, 6)));
        assert!(Trigger::parse_json("nonsense").is_none());
    }

    #[test]
    fn decisions() {
        let songs = vec![
            (
                1,
                Trigger::ProgramChange {
                    channel: 16,
                    value: 1,
                },
            ),
            (
                2,
                Trigger::ProgramChange {
                    channel: 16,
                    value: 2,
                },
            ),
        ];
        let next = Some(Trigger::NoteOn {
            channel: 16,
            note: 60,
        });
        assert_eq!(
            decide(&pc(16, 2), &songs, next, None),
            Decision::GoToSong { song_id: 2 }
        );
        assert_eq!(decide(&pc(16, 9), &songs, next, None), Decision::Unmatched);
        assert_eq!(
            decide(&pc(1, 9), &songs, next, None),
            Decision::Ignore,
            "other channel"
        );
        assert_eq!(
            decide(
                &MidiMsg::NoteOn {
                    channel: 16,
                    note: 60,
                    velocity: 1
                },
                &songs,
                next,
                None
            ),
            Decision::Next
        );
        assert_eq!(
            decide(
                &MidiMsg::NoteOn {
                    channel: 16,
                    note: 61,
                    velocity: 1
                },
                &songs,
                next,
                None
            ),
            Decision::Ignore
        );
        assert_eq!(
            decide(&pc(16, 1), &[], None, None),
            Decision::Ignore,
            "no triggers configured"
        );
    }

    #[test]
    fn dedup_window() {
        let mut d = Dedup::default();
        assert!(d.accept(pc(16, 1), 1000));
        assert!(!d.accept(pc(16, 1), 1200), "repeat inside window");
        assert!(d.accept(pc(16, 2), 1250), "different message");
        assert!(
            d.accept(pc(16, 1), 1300),
            "back to first: last was different"
        );
        assert!(d.accept(pc(16, 1), 1900), "outside window");
    }
}
