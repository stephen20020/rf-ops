//! Domain types shared by the server and the pure logic in this crate.

use serde::{Deserialize, Serialize};

pub type Id = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentKind {
    Receiver,
    ReceiverChannel,
    HandheldTx,
    BodypackTx,
    Capsule,
    Headset,
    Lav,
    IemTx,
    IemTxChannel,
    IemPack,
    GuitarReceiver,
    GuitarPack,
    Battery,
}

impl EquipmentKind {
    pub const ALL: [EquipmentKind; 13] = [
        Self::Receiver,
        Self::ReceiverChannel,
        Self::HandheldTx,
        Self::BodypackTx,
        Self::Capsule,
        Self::Headset,
        Self::Lav,
        Self::IemTx,
        Self::IemTxChannel,
        Self::IemPack,
        Self::GuitarReceiver,
        Self::GuitarPack,
        Self::Battery,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Receiver => "receiver",
            Self::ReceiverChannel => "receiver_channel",
            Self::HandheldTx => "handheld_tx",
            Self::BodypackTx => "bodypack_tx",
            Self::Capsule => "capsule",
            Self::Headset => "headset",
            Self::Lav => "lav",
            Self::IemTx => "iem_tx",
            Self::IemTxChannel => "iem_tx_channel",
            Self::IemPack => "iem_pack",
            Self::GuitarReceiver => "guitar_receiver",
            Self::GuitarPack => "guitar_pack",
            Self::Battery => "battery",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Items that physically travel with a performer and therefore take part in handoffs.
    pub fn is_portable(self) -> bool {
        matches!(
            self,
            Self::HandheldTx
                | Self::BodypackTx
                | Self::Capsule
                | Self::Headset
                | Self::Lav
                | Self::IemPack
                | Self::GuitarPack
        )
    }

    /// Items that are worn/carried and need battery checks.
    pub fn needs_battery(self) -> bool {
        matches!(
            self,
            Self::HandheldTx | Self::BodypackTx | Self::IemPack | Self::GuitarPack
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentStatus {
    Available,
    CheckedOut,
    InUse,
    Faulty,
    Retired,
}

impl EquipmentStatus {
    pub const ALL: [EquipmentStatus; 5] = [
        Self::Available,
        Self::CheckedOut,
        Self::InUse,
        Self::Faulty,
        Self::Retired,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::CheckedOut => "checked_out",
            Self::InUse => "in_use",
            Self::Faulty => "faulty",
            Self::Retired => "retired",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn is_unusable(self) -> bool {
        matches!(self, Self::Faulty | Self::Retired)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KitKind {
    Mic,
    Iem,
    Guitar,
}

impl KitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::Iem => "iem",
            Self::Guitar => "guitar",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mic" => Some(Self::Mic),
            "iem" => Some(Self::Iem),
            "guitar" => Some(Self::Guitar),
            _ => None,
        }
    }
}

/// A set of equipment used together, e.g. receiver channel + handheld + capsule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kit {
    pub id: Id,
    pub kind: KitKind,
    pub name: String,
    /// Fixed (non-portable) side: receiver channel, IEM transmitter channel, guitar receiver.
    pub base_id: Option<Id>,
    /// Transmitter, IEM pack or guitar pack.
    pub carrier_id: Option<Id>,
    /// Capsule (handheld) or headset/lav element (bodypack).
    pub element_id: Option<Id>,
    pub console_channel: Option<i64>,
    pub console_channel_name: Option<String>,
    pub dca: Option<String>,
    pub iem_mix: Option<String>,
    pub is_spare: bool,
}

impl Kit {
    pub fn items(&self) -> impl Iterator<Item = Id> + '_ {
        [self.base_id, self.carrier_id, self.element_id]
            .into_iter()
            .flatten()
    }

    pub fn portable_items(&self) -> impl Iterator<Item = Id> + '_ {
        [self.carrier_id, self.element_id].into_iter().flatten()
    }

    /// Items only one performer can use at a time. An IEM transmitter broadcasts to any number
    /// of packs (e.g. three drummers' packs all on TX1), so it is not exclusive; a mic receiver
    /// channel or guitar receiver is.
    pub fn exclusive_items(&self) -> impl Iterator<Item = Id> + '_ {
        let base = if self.kind == KitKind::Iem {
            None
        } else {
            self.base_id
        };
        [base, self.carrier_id, self.element_id]
            .into_iter()
            .flatten()
    }
}

/// One assignment row.
///
/// `song_id == None` is the show default. A row with `song_id == Some(s)` is an override:
/// if any override rows exist for (performer, song, kind), they replace all default rows of
/// that kind for that performer in that song. An override row with `kit_id == None` means
/// "no kit of this kind in this song".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Id,
    pub performer_id: Id,
    pub kind: KitKind,
    pub kit_id: Option<Id>,
    pub song_id: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Song {
    pub id: Id,
    pub position: i64,
    pub title: String,
}
