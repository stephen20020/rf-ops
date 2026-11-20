//! Show files: one readable YAML file per show, written by hand or by an agent (e.g. Claude
//! Code reading the band's PDFs), loaded with one command or a drag-and-drop in Admin.
//!
//! Everything is by **name** (device labels, people's names, song titles), never ids. `check`
//! returns plain-English problems; errors stop an import, warnings don't. `compile` produces the
//! pieces the server stores. Any stored show can be written back out with `to_yaml`, so files
//! round-trip.
//!
//! ```yaml
//! show: HF Afternoon Friday
//! event: Homestead Fair 2026
//! timezone: America/Chicago
//! day: 2026-06-19
//! set: afternoon
//! concert: "14:00"
//! call: "13:00"            # optional; defaults to an hour before the concert
//! follows: null            # optional: the show this one carries over from (same day)
//!
//! people:
//!   - Helen                # or { name: Helen, zone: SL, notes: "lead vox" }
//!   - { name: Shosh, zone: SR }
//!
//! devices:                 # optional: only ones not already in RF Ops
//!   - { label: Gabe's Shure, kind: mic }
//!
//! mixes:                   # IEM transmitter → mix for this show
//!   TX1: ME 1
//!
//! songs:
//!   - { title: Walk-in, pc: 1 }
//!   - title: I Will Follow Jesus / Lace Up Your Boots
//!     pc: 5
//!     passes:
//!       - "RX10: Helen -> Shosh"
//!
//! plot:
//!   default:               # who has each device unless a song says otherwise
//!     Red: Helen
//!     RX10: Helen
//!   I Will Follow Jesus / Lace Up Your Boots:
//!     Tan: Judah
//!     Blue: nobody
//! ```

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::plan::{self, Kind};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShowFile {
    pub show: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<String>,
    #[serde(default, rename = "set", skip_serializing_if = "Option::is_none")]
    pub set_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concert: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<PersonEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<DeviceEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mixes: BTreeMap<String, String>,
    #[serde(default)]
    pub songs: Vec<SongEntry>,
    /// Column name ("default" or a song title) → device label → person name or "nobody".
    #[serde(default)]
    pub plot: BTreeMap<String, BTreeMap<String, Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PersonEntry {
    Name(String),
    Full(Person),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Person {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Not here for this show.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub absent: bool,
}

impl PersonEntry {
    pub fn person(&self) -> Person {
        match self {
            PersonEntry::Name(n) => Person {
                name: n.clone(),
                ..Default::default()
            },
            PersonEntry::Full(p) => p.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeviceEntry {
    pub label: String,
    pub kind: Kind,
    /// Console channel (mics/guitar) or IEM transmitter label ("TX1").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spare: bool,
    /// A guest's own device ("Chandler's pack"): name of the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SongEntry {
    pub title: String,
    /// MIDI Program Change number Playback sends for this song.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pc: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
    /// "RX10: Helen -> Shosh", in the order they happen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// What the server already has (devices by label).
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub devices: BTreeMap<String, Kind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub level: Level,
    /// Where: "line 12", "song 3 (I Will Follow Jesus)", "plot → default → Red".
    pub at: String,
    pub message: String,
}

fn err(at: impl Into<String>, m: impl Into<String>) -> Problem {
    Problem {
        level: Level::Error,
        at: at.into(),
        message: m.into(),
    }
}
fn warn(at: impl Into<String>, m: impl Into<String>) -> Problem {
    Problem {
        level: Level::Warning,
        at: at.into(),
        message: m.into(),
    }
}

/// A parsed pass: device, from, to (None = nobody).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassSpec {
    pub device: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// What to store: everything resolved to names, checked, in order.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Compiled {
    pub people: Vec<Person>,
    pub new_devices: Vec<DeviceEntry>,
    pub songs: Vec<SongEntry>,
    /// (device label, song index or None for default) → person name or None.
    pub cells: Vec<(String, Option<usize>, Option<String>)>,
    /// (song index, passes in order).
    pub passes: Vec<(usize, Vec<PassSpec>)>,
}

fn nobody(s: &str) -> bool {
    matches!(
        s.trim().to_lowercase().as_str(),
        "" | "nobody" | "none" | "-" | "—" | "no one" | "off"
    )
}

/// Closest known name for "did you mean …?" (simple edit distance, case-insensitive).
fn closest<'a>(want: &str, among: impl Iterator<Item = &'a String>) -> Option<&'a String> {
    fn dist(a: &str, b: &str) -> usize {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut cur = vec![i; b.len() + 1];
            for j in 1..=b.len() {
                let c = usize::from(a[i - 1] != b[j - 1]);
                cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + c);
            }
            prev = cur;
        }
        prev[b.len()]
    }
    let w = want.to_lowercase();
    among
        .map(|c| (dist(&w, &c.to_lowercase()), c))
        .filter(|(d, c)| *d <= 2.max(c.len() / 4))
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn hint(want: &str, among: &BTreeSet<String>) -> String {
    closest(want, among.iter())
        .map(|c| format!(" Did you mean \"{c}\"?"))
        .unwrap_or_default()
}

pub fn parse(yaml: &str) -> Result<ShowFile, Problem> {
    serde_yaml::from_str::<ShowFile>(yaml).map_err(|e| {
        let at = e
            .location()
            .map(|l| format!("line {}", l.line()))
            .unwrap_or_else(|| "file".into());
        err(at, format!("The file isn't valid: {e}"))
    })
}

fn parse_pass(s: &str) -> Option<PassSpec> {
    let (device, rest) = s.split_once(':')?;
    let rest = rest.replace('→', "->");
    let (from, to) = rest.split_once("->")?;
    let person = |x: &str| (!nobody(x)).then(|| x.trim().to_string());
    Some(PassSpec {
        device: device.trim().to_string(),
        from: person(from),
        to: person(to),
    })
}

fn valid_time(t: &str) -> bool {
    let Some((h, m)) = t.split_once(':') else { return false };
    matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60 && m.to_string().len() <= 2 && h.to_string().len() <= 2)
}

/// Check a show file against the inventory, and compile it if there are no errors.
pub fn check(f: &ShowFile, inv: &Inventory) -> (Vec<Problem>, Option<Compiled>) {
    let mut probs = Vec::new();
    if f.show.trim().is_empty() {
        probs.push(err("show", "The show needs a name (show: …)."));
    }
    for (what, t) in [("concert", &f.concert), ("call", &f.call)] {
        if let Some(t) = t {
            if !valid_time(t) {
                probs.push(err(what, format!("\"{t}\" isn't a time; write it like \"19:30\".")));
            }
        }
    }
    if let Some(d) = &f.day {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            probs.push(err("day", format!("\"{d}\" isn't a date; write it like 2026-06-19.")));
        }
    }

    // People.
    let mut people: Vec<Person> = Vec::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    for p in &f.people {
        let p = p.person();
        if p.name.trim().is_empty() {
            probs.push(err("people", "A person has no name."));
            continue;
        }
        if !names.insert(p.name.trim().to_string()) {
            probs.push(err("people", format!("{} is listed twice.", p.name)));
        }
        people.push(p);
    }

    // Devices: existing ones plus new ones in the file.
    let mut kinds: BTreeMap<String, Kind> = inv.devices.clone();
    for d in &f.devices {
        if kinds.contains_key(&d.label) && !inv.devices.contains_key(&d.label) {
            probs.push(err("devices", format!("Device {} is listed twice.", d.label)));
        }
        if let Some(o) = &d.owner {
            if !names.contains(o) {
                probs.push(err(
                    format!("devices → {}", d.label),
                    format!("Owner {o} isn't in people.{}", hint(o, &names)),
                ));
            }
        }
        kinds.insert(d.label.clone(), d.kind);
    }
    let labels: BTreeSet<String> = kinds.keys().cloned().collect();

    // Songs.
    let mut titles: BTreeMap<String, usize> = BTreeMap::new();
    let mut pcs: BTreeMap<u8, String> = BTreeMap::new();
    for (i, s) in f.songs.iter().enumerate() {
        let at = format!("song {} ({})", i + 1, s.title);
        if s.title.trim().is_empty() {
            probs.push(err(format!("song {}", i + 1), "A song has no title."));
        }
        if titles.insert(s.title.clone(), i).is_some() {
            probs.push(err(
                at.clone(),
                format!("Two songs are called \"{}\"; titles must differ (add \" (reprise)\").", s.title),
            ));
        }
        if let Some(pc) = s.pc {
            if pc > 127 {
                probs.push(err(at.clone(), "Program Change numbers go from 0 to 127."));
            } else if let Some(other) = pcs.insert(pc, s.title.clone()) {
                probs.push(err(
                    at.clone(),
                    format!("PC {pc} is also used by \"{other}\": Playback can't tell them apart."),
                ));
            }
        }
    }
    if f.songs.is_empty() {
        probs.push(warn("songs", "No songs yet: the plot only has its default column."));
    }

    // Plot.
    let check_person = |who: &str, at: &str, probs: &mut Vec<Problem>| {
        if !names.contains(who) {
            probs.push(err(
                at.to_string(),
                format!("{who} isn't in people.{}", hint(who, &names)),
            ));
        }
    };
    let mut cells = Vec::new();
    for (col, row) in &f.plot {
        let song = if col.eq_ignore_ascii_case("default") {
            None
        } else {
            match titles.get(col) {
                Some(i) => Some(*i),
                None => {
                    let known: BTreeSet<String> = titles.keys().cloned().collect();
                    probs.push(err(
                        format!("plot → {col}"),
                        format!("There's no song called \"{col}\" in songs.{}", hint(col, &known)),
                    ));
                    continue;
                }
            }
        };
        for (dev, who) in row {
            let at = format!("plot → {col} → {dev}");
            if !kinds.contains_key(dev) {
                probs.push(err(
                    at.clone(),
                    format!(
                        "There's no device called \"{dev}\". Add it under devices: or check the spelling.{}",
                        hint(dev, &labels)
                    ),
                ));
                continue;
            }
            let who = who.as_deref().filter(|w| !nobody(w)).map(|w| w.trim().to_string());
            if let Some(w) = &who {
                check_person(w, &at, &mut probs);
            }
            cells.push((dev.clone(), song, who));
        }
    }

    // Passes.
    let mut passes = Vec::new();
    for (i, s) in f.songs.iter().enumerate() {
        let at = format!("song {} ({}) → passes", i + 1, s.title);
        let mut list = Vec::new();
        for raw in &s.passes {
            let Some(p) = parse_pass(raw) else {
                probs.push(err(at.clone(), format!("\"{raw}\" isn't a pass; write it like \"RX10: Helen -> Shosh\".")));
                continue;
            };
            if !kinds.contains_key(&p.device) {
                probs.push(err(at.clone(), format!("There's no device called \"{}\".{}", p.device, hint(&p.device, &labels))));
                continue;
            }
            for w in [&p.from, &p.to].into_iter().flatten() {
                check_person(w, &at, &mut probs);
            }
            list.push(p);
        }
        if !list.is_empty() {
            passes.push((i, list));
        }
    }
    if probs.iter().any(|p| p.level == Level::Error) {
        return (probs, None);
    }

    // Semantics: build a temporary plan and let the core rules find conflicts.
    let mut dev_ids: BTreeMap<String, plan::Id> = BTreeMap::new();
    let mut st = plan::ShowState::default();
    for (n, (label, kind)) in kinds.iter().enumerate() {
        let id = n as plan::Id + 1;
        dev_ids.insert(label.clone(), id);
        st.devices.push(plan::Device {
            id,
            label: label.clone(),
            kind: *kind,
            base: None,
            element: None,
            is_spare: false,
            guest_owner: None,
        });
    }
    let person_ids: BTreeMap<String, plan::Id> =
        people.iter().enumerate().map(|(n, p)| (p.name.clone(), n as plan::Id + 1)).collect();
    let name_of = |id: plan::Id| {
        person_ids
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    };
    let label_of = |id: plan::Id| {
        dev_ids
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    };
    st.songs = (0..f.songs.len())
        .map(|i| plan::Song { id: i as plan::Id + 1, skipped: f.songs[i].skip })
        .collect();
    for p in &people {
        if p.absent {
            st.absent.insert(person_ids[&p.name]);
        }
    }
    for (dev, song, who) in &cells {
        st.cells.insert(
            (dev_ids[dev], song.map(|i| i as plan::Id + 1)),
            who.as_ref().map(|w| person_ids[w]),
        );
    }
    let mut pid = 1;
    for (i, list) in &passes {
        let mut seq_by_dev: BTreeMap<String, u32> = BTreeMap::new();
        for p in list {
            let seq = seq_by_dev.entry(p.device.clone()).or_insert(0);
            *seq += 1;
            st.passes.push(plan::Pass {
                id: pid,
                device: dev_ids[&p.device],
                song: *i as plan::Id + 1,
                seq: *seq,
                to: p.to.as_ref().map(|w| person_ids[w]),
            });
            pid += 1;
        }
    }
    let song_name = |sid: plan::Id| {
        let i = sid as usize - 1;
        format!("song {} ({})", i + 1, f.songs[i].title)
    };
    for (i, list) in &passes {
        // The "from" side must be whoever has the device just before the pass.
        let sid = *i as plan::Id + 1;
        let (chains, _) = st.chains(sid, 1);
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for p in list {
            let k = seen.entry(p.device.clone()).or_insert(0);
            let c = &chains[&dev_ids[&p.device]];
            let before = c.holders[*k].map(name_of);
            if before != p.from {
                probs.push(err(
                    format!("{} → passes", song_name(sid)),
                    format!(
                        "\"{}: {} -> {}\": at that point {} has {}. Fix the plot or the pass.",
                        p.device,
                        p.from.as_deref().unwrap_or("nobody"),
                        p.to.as_deref().unwrap_or("nobody"),
                        before.as_deref().unwrap_or("nobody"),
                        p.device
                    ),
                ));
            }
            *k += 1;
        }
    }
    for s in &st.songs {
        let (chains, _) = st.chains(s.id, 1);
        for c in st_conflicts(&st, s.id, &chains) {
            let at = song_name(s.id);
            let labels = |d: &[plan::Id]| d.iter().map(|x| label_of(*x)).collect::<Vec<_>>().join(", ");
            match c {
                plan::Conflict::TwoPacks { performer, kind, devices, .. } => probs.push(err(
                    at,
                    format!("{} has two {} at once: {}.", name_of(performer), if kind == Kind::Iem { "IEM packs" } else { "guitar packs" }, labels(&devices)),
                )),
                plan::Conflict::TwoMics { performer, devices, .. } => probs.push(warn(
                    at,
                    format!("{} has two mics: {} (fine if intended).", name_of(performer), labels(&devices)),
                )),
                plan::Conflict::AbsentPlanned { device, performer, .. } => probs.push(warn(
                    at,
                    format!("{} is marked absent but planned on {}.", name_of(performer), label_of(device)),
                )),
                _ => {}
            }
        }
    }
    if probs.iter().any(|p| p.level == Level::Error) {
        return (probs, None);
    }
    let new_devices = f
        .devices
        .iter()
        .filter(|d| !inv.devices.contains_key(&d.label))
        .cloned()
        .collect();
    (
        probs,
        Some(Compiled {
            people,
            new_devices,
            songs: f.songs.clone(),
            cells,
            passes,
        }),
    )
}

fn st_conflicts(
    st: &plan::ShowState,
    song: plan::Id,
    chains: &BTreeMap<plan::Id, plan::Chain>,
) -> Vec<plan::Conflict> {
    st.conflicts_for(song, chains)
}

/// Write a show back out as a file (for editing, copying or keeping).
pub fn to_yaml(f: &ShowFile) -> String {
    serde_yaml::to_string(f).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
show: HF Afternoon Friday
event: Homestead Fair 2026
timezone: America/Chicago
day: 2026-06-19
set: afternoon
concert: "14:00"
people:
  - Helen
  - { name: Shosh, zone: SR }
  - Judah
songs:
  - { title: Walk-in, pc: 1 }
  - title: I Will Follow Jesus / Lace Up Your Boots
    pc: 5
    passes:
      - "RX10: Helen -> Shosh"
  - { title: Closer, pc: 6 }
plot:
  default:
    Red: Helen
    RX10: Helen
  Closer:
    Tan: Judah
"#;

    fn inv() -> Inventory {
        Inventory {
            devices: BTreeMap::from([
                ("Red".into(), Kind::Mic),
                ("Tan".into(), Kind::Mic),
                ("RX10".into(), Kind::Iem),
                ("RX9".into(), Kind::Iem),
            ]),
        }
    }

    #[test]
    fn sample_checks_clean_and_compiles() {
        let f = parse(SAMPLE).unwrap();
        let (probs, c) = check(&f, &inv());
        assert!(probs.is_empty(), "{probs:?}");
        let c = c.unwrap();
        assert_eq!(c.songs.len(), 3);
        assert_eq!(c.passes[0].0, 1);
        assert_eq!(c.passes[0].1[0], PassSpec { device: "RX10".into(), from: Some("Helen".into()), to: Some("Shosh".into()) });
    }

    #[test]
    fn round_trips() {
        let f = parse(SAMPLE).unwrap();
        let again = parse(&to_yaml(&f)).unwrap();
        assert_eq!(f, again);
    }

    #[test]
    fn plain_english_errors_with_suggestions() {
        let bad = SAMPLE
            .replace("Red: Helen", "Redd: Helen")
            .replace("Tan: Judah", "Tan: Juda")
            .replace("{ title: Closer, pc: 6 }", "{ title: Closer, pc: 5 }");
        let (probs, c) = check(&parse(&bad).unwrap(), &inv());
        assert!(c.is_none());
        let text: Vec<String> = probs.iter().map(|p| format!("{}: {}", p.at, p.message)).collect();
        let all = text.join("\n");
        assert!(all.contains("no device called \"Redd\"") && all.contains("Did you mean \"Red\""), "{all}");
        assert!(all.contains("Juda isn't in people") && all.contains("\"Judah\""), "{all}");
        assert!(all.contains("PC 5 is also used"), "{all}");
    }

    #[test]
    fn pass_from_must_match_who_has_it() {
        let bad = SAMPLE.replace("RX10: Helen -> Shosh", "RX10: Judah -> Shosh");
        let (probs, _) = check(&parse(&bad).unwrap(), &inv());
        assert!(probs.iter().any(|p| p.message.contains("at that point Helen has RX10")), "{probs:?}");
    }

    #[test]
    fn two_iem_packs_is_an_error_two_mics_a_warning() {
        let two = SAMPLE.replace("    RX10: Helen\n", "    RX10: Helen\n    RX9: Helen\n    Tan: Helen\n");
        let (probs, c) = check(&parse(&two).unwrap(), &inv());
        assert!(c.is_none());
        assert!(probs.iter().any(|p| p.level == Level::Error && p.message.contains("two IEM packs")));
        assert!(probs.iter().any(|p| p.level == Level::Warning && p.message.contains("two mics")));
    }

    #[test]
    fn unknown_fields_and_bad_yaml_say_where() {
        let e = parse("show: X\nsongz: []\n").unwrap_err();
        assert!(e.message.contains("songz"), "{e:?}");
        let e = parse("show: [unclosed\n").unwrap_err();
        assert!(e.at.starts_with("line"), "{e:?}");
    }

    #[test]
    fn new_devices_and_nobody() {
        let f = SAMPLE.replace("plot:\n", "devices:\n  - { label: Gabe's Shure, kind: mic }\nplot:\n")
            .replace("    Tan: Judah\n", "    Tan: Judah\n    Gabe's Shure: nobody\n");
        let (probs, c) = check(&parse(&f).unwrap(), &inv());
        assert!(probs.is_empty(), "{probs:?}");
        let c = c.unwrap();
        assert_eq!(c.new_devices.len(), 1);
        assert!(c.cells.contains(&("Gabe's Shure".into(), Some(2), None)));
    }
}
