//! Loads a show from the database into `rfops_core::plan::ShowState`, and writes holdings and
//! visits back. All the rules live in the core crate; this file is only storage.

use std::collections::{BTreeMap, BTreeSet};

use rfops_core::plan::{
    self, Device, Holder, Holding, Id, Kind, Pass, ShowState, Song, SubKind, SubRange, SubState,
    Substitution, Usability, Visit,
};
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Synthetic equipment ids carrying a device's own status (suspect / out / missing).
const DEVICE_STATUS_BASE: Id = 1_000_000_000;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Names {
    pub devices: BTreeMap<Id, String>,
    pub people: BTreeMap<Id, String>,
    pub songs: BTreeMap<Id, String>,
    pub equipment: BTreeMap<Id, String>,
}

pub fn kind_from(s: &str) -> Kind {
    match s {
        "iem" => Kind::Iem,
        "guitar" => Kind::Guitar,
        _ => Kind::Mic,
    }
}

pub fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Mic => "mic",
        Kind::Iem => "iem",
        Kind::Guitar => "guitar",
    }
}

fn usability(s: &str) -> Usability {
    match s {
        "suspect" => Usability::Suspect,
        "out" | "out_of_service" | "faulty" | "retired" => Usability::Out,
        "missing" => Usability::Missing,
        _ => Usability::Ok,
    }
}

fn worst(a: Usability, b: Usability) -> Usability {
    let r = |u: Usability| match u {
        Usability::Ok => 0,
        Usability::Suspect => 1,
        Usability::Out => 2,
        Usability::Missing => 3,
    };
    if r(b) > r(a) {
        b
    } else {
        a
    }
}

pub fn sub_kind(s: &str) -> SubKind {
    match s {
        "pull" => SubKind::Pull,
        "reassign" => SubKind::Reassign,
        "guest" => SubKind::Guest,
        "tx_swap" => SubKind::TxSwap,
        _ => SubKind::Swap,
    }
}
pub fn sub_range(s: &str) -> SubRange {
    match s {
        "this_song" => SubRange::ThisSong,
        "rest_of_show" => SubRange::RestOfShow,
        _ => SubRange::UntilFixed,
    }
}
pub fn sub_state(s: &str) -> SubState {
    match s {
        "planned" => SubState::Planned,
        "recorded" => SubState::Recorded,
        "foh_confirmed" => SubState::FohConfirmed,
        "reversed" => SubState::Reversed,
        "return_recorded" => SubState::ReturnRecorded,
        "returned" => SubState::Returned,
        _ => SubState::InHand,
    }
}

type DeviceRow = (Id, String, String, Option<Id>, Option<Id>, Option<Id>, bool, Option<Id>, String);
type HoldingRow = (Id, String, Option<Id>, Option<Id>, Option<Id>);
type SubRow = (
    Id,
    String,
    Option<Id>,
    Option<Id>,
    Option<Id>,
    Option<Id>,
    Option<Id>,
    bool,
    String,
    String,
    i64,
    Option<Id>,
    Option<i64>,
);

/// Planned holder of each device at the first song of a show (for carry-over, R4).
async fn first_song_plan(
    pool: &SqlitePool,
    show_id: Id,
) -> Result<Option<BTreeMap<Id, Option<Id>>>, sqlx::Error> {
    let first: Option<Id> = sqlx::query_scalar(
        "SELECT id FROM songs WHERE show_id = ? AND skipped = 0 ORDER BY position LIMIT 1",
    )
    .bind(show_id)
    .fetch_optional(pool)
    .await?;
    let Some(first) = first else { return Ok(None) };
    let rows: Vec<(Id, i64, Option<Id>)> = sqlx::query_as(
        "SELECT device_id, song_key, performer_id FROM plot_cells WHERE show_id = ? AND song_key IN (0, ?)",
    )
    .bind(show_id)
    .bind(first)
    .fetch_all(pool)
    .await?;
    let mut m: BTreeMap<Id, Option<Id>> = BTreeMap::new();
    for (d, k, p) in rows.iter().filter(|r| r.1 == 0) {
        let _ = k;
        m.insert(*d, *p);
    }
    for (d, _, p) in rows.iter().filter(|r| r.1 != 0) {
        m.insert(*d, *p);
    }
    Ok(Some(m))
}

pub async fn load(pool: &SqlitePool, show_id: Id) -> Result<(ShowState, Names), sqlx::Error> {
    let mut st = ShowState::default();
    let mut names = Names::default();

    let eq: Vec<(Id, String, String)> = sqlx::query_as("SELECT id, label, status FROM equipment")
        .fetch_all(pool)
        .await?;
    for (id, label, status) in &eq {
        names.equipment.insert(*id, label.clone());
        let u = usability(status);
        if u != Usability::Ok {
            st.equipment.insert(*id, u);
        }
    }

    let devs: Vec<DeviceRow> = sqlx::query_as(
        "SELECT id, label, kind, carrier_id, base_id, element_id, is_spare, guest_owner_id, status
         FROM devices WHERE active = 1 ORDER BY board_order, id",
    )
    .fetch_all(pool)
    .await?;
    for (id, label, kind, carrier, base, element, spare, owner, status) in devs {
        names.devices.insert(id, label.clone());
        // A device's usability = worst of its own status and its carrier's.
        let key = DEVICE_STATUS_BASE + id;
        let carrier_u = carrier.and_then(|c| st.equipment.get(&c).copied()).unwrap_or_default();
        let u = worst(usability(&status), carrier_u);
        if u != Usability::Ok {
            st.equipment.insert(key, u);
        }
        st.carriers.insert(id, key);
        st.devices.push(Device {
            id,
            label,
            kind: kind_from(&kind),
            base,
            element,
            is_spare: spare,
            guest_owner: owner,
        });
    }

    let people: Vec<(Id, String)> = sqlx::query_as("SELECT id, name FROM performers")
        .fetch_all(pool)
        .await?;
    names.people = people.into_iter().collect();

    let songs: Vec<(Id, String, bool)> =
        sqlx::query_as("SELECT id, title, skipped FROM songs WHERE show_id = ? ORDER BY position")
            .bind(show_id)
            .fetch_all(pool)
            .await?;
    for (id, title, skipped) in songs {
        names.songs.insert(id, title);
        st.songs.push(Song { id, skipped });
    }

    let cells: Vec<(Id, i64, Option<Id>, Option<Id>)> = sqlx::query_as(
        "SELECT device_id, song_key, performer_id, element_id FROM plot_cells WHERE show_id = ?",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    for (d, k, p, e) in cells {
        let song = (k != 0).then_some(k);
        st.cells.insert((d, song), p);
        if let Some(e) = e {
            st.cell_elements.insert((d, song), e);
        }
    }

    let passes: Vec<(Id, Id, Id, i64, Option<Id>)> = sqlx::query_as(
        "SELECT id, device_id, song_id, seq, to_performer_id FROM passes WHERE show_id = ?",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    st.passes = passes
        .into_iter()
        .map(|(id, device, song, seq, to)| Pass { id, device, song, seq: seq as u32, to })
        .collect();

    let marks: Vec<(Id, i64)> = sqlx::query_as(
        "SELECT m.pass_id, m.visit_no FROM pass_marks m JOIN passes p ON p.id = m.pass_id WHERE p.show_id = ?",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    st.pass_marks = marks.into_iter().map(|(p, v)| (p, v as u32)).collect();

    let visits: Vec<(i64, Id)> = sqlx::query_as(
        "SELECT visit_no, song_id FROM song_visits WHERE show_id = ? ORDER BY visit_no",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    st.visits = visits.into_iter().map(|(no, song)| Visit { no: no as u32, song }).collect();

    let subs: Vec<SubRow> = sqlx::query_as(
        "SELECT id, kind, performer_id, failed_id, replacement_id, failed_base_id, replacement_base_id,
                retune, range, state, from_visit, starts_at_song_id, ended_at_visit
         FROM substitutions WHERE show_id = ? ORDER BY id",
    )
    .bind(show_id)
    .fetch_all(pool)
    .await?;
    st.subs = subs
        .into_iter()
        .map(|r| Substitution {
            id: r.0,
            kind: sub_kind(&r.1),
            performer: r.2,
            failed: r.3,
            replacement: r.4,
            failed_base: r.5,
            replacement_base: r.6,
            retune: r.7,
            range: sub_range(&r.8),
            state: sub_state(&r.9),
            from_visit: r.10 as u32,
            starts_at_song: r.11,
            ended_at_visit: r.12.map(|v| v as u32),
        })
        .collect();

    let holdings: Vec<HoldingRow> = sqlx::query_as(
        "SELECT device_id, holder_kind, performer_id, element_id, tuned_base_id FROM holdings",
    )
    .fetch_all(pool)
    .await?;
    for (d, kind, p, e, t) in holdings {
        let holder = match (kind.as_str(), p) {
            ("person", Some(p)) => Holder::Person(p),
            ("owner", _) => Holder::Owner,
            _ => Holder::Stock,
        };
        st.holdings.insert(d, Holding { holder, element: e, tuned_base: t });
    }

    let mixes: Vec<(Id, String)> =
        sqlx::query_as("SELECT base_id, mix FROM show_bases WHERE show_id = ?")
            .bind(show_id)
            .fetch_all(pool)
            .await?;
    st.base_mix = mixes.into_iter().collect();

    let absent: Vec<Id> =
        sqlx::query_scalar("SELECT performer_id FROM performer_absences WHERE show_id = ?")
            .bind(show_id)
            .fetch_all(pool)
            .await?;
    st.absent = absent.into_iter().collect();

    let left: Vec<(i64, Id)> =
        sqlx::query_as("SELECT visit_no, device_id FROM handoff_left WHERE show_id = ?")
            .bind(show_id)
            .fetch_all(pool)
            .await?;
    st.left = left.into_iter().map(|(v, d)| (v as u32, d)).collect::<BTreeSet<_>>();

    // R4: the same-day show that follows this one.
    let next: Option<Id> = sqlx::query_scalar(
        "SELECT n.id FROM shows n JOIN shows s ON s.id = ?
         WHERE n.follows_show_id = s.id AND (n.day IS s.day) ORDER BY n.id LIMIT 1",
    )
    .bind(show_id)
    .fetch_optional(pool)
    .await?;
    if let Some(n) = next {
        st.next_show_first = first_song_plan(pool, n).await?;
    }
    Ok((st, names))
}

/// Record a song change as a new visit (every change is a visit; going back redoes the song).
pub async fn record_visit(
    tx: &mut Transaction<'_, Sqlite>,
    show_id: Id,
    song_id: Option<Id>,
    via: &str,
) -> Result<(), sqlx::Error> {
    let Some(song) = song_id else { return Ok(()) };
    let last: Option<(i64, Id)> = sqlx::query_as(
        "SELECT visit_no, song_id FROM song_visits WHERE show_id = ? ORDER BY visit_no DESC LIMIT 1",
    )
    .bind(show_id)
    .fetch_optional(&mut **tx)
    .await?;
    if last.is_some_and(|(_, s)| s == song) {
        return Ok(()); // same song re-selected: still the same visit
    }
    sqlx::query(
        "INSERT INTO song_visits (show_id, visit_no, song_id, started_at, via) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(show_id)
    .bind(last.map(|(n, _)| n + 1).unwrap_or(1))
    .bind(song)
    .bind(crate::db::now())
    .bind(via)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Write holding changes (one row per device).
pub async fn write_holdings(
    tx: &mut Transaction<'_, Sqlite>,
    moves: &plan::Moves,
    by: &str,
) -> Result<(), sqlx::Error> {
    for (d, h) in moves {
        let (kind, p) = match h.holder {
            Holder::Stock => ("stock", None),
            Holder::Person(p) => ("person", Some(p)),
            Holder::Owner => ("owner", None),
        };
        sqlx::query(
            "INSERT INTO holdings (device_id, holder_kind, performer_id, element_id, tuned_base_id, since, by_name)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(device_id) DO UPDATE SET holder_kind = excluded.holder_kind,
               performer_id = excluded.performer_id, element_id = excluded.element_id,
               tuned_base_id = excluded.tuned_base_id, since = excluded.since, by_name = excluded.by_name",
        )
        .bind(d)
        .bind(kind)
        .bind(p)
        .bind(h.element)
        .bind(h.tuned_base)
        .bind(crate::db::now())
        .bind(by)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
