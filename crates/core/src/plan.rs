//! The core rules (docs/data-model.md v3.1 + owner decisions R1–R8), as pure functions.
//!
//! One idea: **a handoff is any difference between who should have a device (the plan) and who
//! has it (the holding).** The plan is computed per song *visit* and per pass *phase*:
//!
//! 1. Plot cells give each device a holder for a song (or the Default column).
//! 2. Passes split a song into phases: the holder after pass k is that pass's `to`.
//! 3. Substitutions (swap, pull, reassign, guest, TX swap) move a device's whole phase chain onto
//!    another device, never overwriting someone else's chain (that is a conflict instead).
//! 4. Unusable devices (suspect / out / missing carrier, base or element) are never handed out.
//!
//! Everything here is deterministic and side-effect free; the server stores the inputs and
//! applies the outputs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub type Id = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mic,
    Iem,
    Guitar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Usability {
    #[default]
    Ok,
    Suspect,
    Out,
    Missing,
}

impl Usability {
    pub fn usable(self) -> bool {
        self == Usability::Ok
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: Id,
    pub label: String,
    pub kind: Kind,
    /// Mic: receiver channel. IEM: transmitter (may be shared). Guitar: receiver. Equipment id.
    pub base: Option<Id>,
    /// Default capsule / headset (equipment id).
    pub element: Option<Id>,
    pub is_spare: bool,
    /// Guest devices belong to one performer ("Chandler's pack").
    pub guest_owner: Option<Id>,
}

/// Who physically has a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "id")]
pub enum Holder {
    Stock,
    Person(Id),
    /// A guest device given back to its owner (gone home).
    Owner,
}

impl Holder {
    pub fn person(self) -> Option<Id> {
        match self {
            Holder::Person(p) => Some(p),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Holding {
    pub holder: Holder,
    /// Element fitted now (None = the device's default).
    pub element: Option<Id>,
    /// For IEM packs: the TX the pack is synced to now (None = its own base).
    pub tuned_base: Option<Id>,
}

impl Default for Holding {
    fn default() -> Self {
        Holding {
            holder: Holder::Stock,
            element: None,
            tuned_base: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pass {
    pub id: Id,
    pub device: Id,
    pub song: Id,
    /// 1, 2, … within that device and song.
    pub seq: u32,
    pub to: Option<Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubKind {
    /// Performer moves from the failed device to the replacement (R1 default: until fixed).
    Swap,
    /// Failed device taken away with no replacement.
    Pull,
    /// Nothing broken: performer uses another device (how an A2 resolves a conflict, R7).
    Reassign,
    /// Performer uses their own guest device instead of a planned one.
    Guest,
    /// Every pack on the failed TX uses the replacement TX.
    TxSwap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubRange {
    /// Only the visit it was made in.
    ThisSong,
    /// R1: until someone marks it returned.
    UntilFixed,
    /// Every later song for every holder of the failed device.
    RestOfShow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubState {
    /// Made for a later song (Extend / pick ahead): no steps; normal handoffs at the boundary.
    Planned,
    /// Tapped, spare not yet on the performer: its devices are left out of handoff lists.
    Recorded,
    InHand,
    FohConfirmed,
    /// Undone after In hand: no longer applies.
    Reversed,
    /// "Return Helen to Red" tapped, not yet in hand.
    ReturnRecorded,
    /// Ended by Return (see `ended_at_visit`).
    Returned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Substitution {
    pub id: Id,
    pub kind: SubKind,
    /// Whose device changes. None = every holder (rest of show, TX swap).
    pub performer: Option<Id>,
    pub failed: Option<Id>,
    pub replacement: Option<Id>,
    /// TX swap: from / to base. Swap: `retune` puts the replacement on the failed device's TX (R6).
    pub failed_base: Option<Id>,
    pub replacement_base: Option<Id>,
    pub retune: bool,
    pub range: SubRange,
    pub state: SubState,
    /// Visit number it was made in.
    pub from_visit: u32,
    /// For a planned (future) substitution: the first song it applies to.
    pub starts_at_song: Option<Id>,
    /// Set by Return: applies only to visits before this one.
    pub ended_at_visit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub id: Id,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub no: u32,
    pub song: Id,
}

/// Everything the rules need about one show (one scope: real or a practice copy).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShowState {
    pub devices: Vec<Device>,
    /// In setlist order.
    pub songs: Vec<Song>,
    /// (device, song) → performer; song None = Default column; performer None = nobody.
    pub cells: BTreeMap<(Id, Option<Id>), Option<Id>>,
    /// (device, song) → element for that song (capsule swap planned in the plot).
    pub cell_elements: BTreeMap<(Id, Option<Id>), Id>,
    pub passes: Vec<Pass>,
    /// (pass id, visit no) marked done.
    pub pass_marks: BTreeSet<(Id, u32)>,
    /// In creation order.
    pub subs: Vec<Substitution>,
    pub visits: Vec<Visit>,
    pub holdings: BTreeMap<Id, Holding>,
    /// Usability of equipment by id (carriers, bases, elements). Missing = Ok.
    pub equipment: BTreeMap<Id, Usability>,
    /// Device id → carrier equipment id (for usability).
    pub carriers: BTreeMap<Id, Id>,
    /// TX base → mix label for this show.
    pub base_mix: BTreeMap<Id, String>,
    pub absent: BTreeSet<Id>,
    /// (visit, device) handoffs the A2 chose to leave for that visit.
    pub left: BTreeSet<(u32, Id)>,
    /// Receiver channel → receiver unit (to flag "same receiver" swap candidates).
    pub base_parents: BTreeMap<Id, Id>,
    /// R4: planned holder at the first song of the linked same-day next show, if any.
    pub next_show_first: Option<BTreeMap<Id, Option<Id>>>,
}

// ---------------------------------------------------------------- plan with phases

/// One device's holders through a song: index k = after k passes.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub holders: Vec<Option<Id>>,
    /// Pass ids in order; `passes[k]` moves holders[k] → holders[k+1].
    pub passes: Vec<Id>,
    /// Which substitution put this chain here (None = the plot).
    pub via: Option<Id>,
}

impl Chain {
    fn empty() -> Self {
        Chain {
            holders: vec![None],
            passes: vec![],
            via: None,
        }
    }
    fn is_empty(&self) -> bool {
        self.holders.iter().all(Option::is_none)
    }
    fn mentions(&self, p: Id) -> bool {
        self.holders.contains(&Some(p))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Conflict {
    /// A substitution wanted a device someone else is planned on in this song (never overwritten).
    ReplacementTaken { song: Id, sub: Id, device: Id, planned_for: Option<Id> },
    /// A device that can't be used is planned for someone (R2).
    UnusablePlanned { song: Id, device: Id, performer: Id },
    TwoPacks { song: Id, performer: Id, kind: Kind, devices: Vec<Id> },
    TwoMics { song: Id, performer: Id, devices: Vec<Id> },
    ElementTwice { song: Id, element: Id, devices: Vec<Id> },
    AbsentPlanned { song: Id, device: Id, performer: Id },
    /// A pass on a pulled device has nobody to give from.
    PassOnPulled { song: Id, device: Id, to: Option<Id> },
    /// Two TX swaps onto one TX, or onto a TX already feeding another pack.
    TxTaken { song: Id, base: Id, sub: Id },
}

impl ShowState {
    pub fn device(&self, id: Id) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn position(&self, song: Id) -> Option<usize> {
        self.songs.iter().position(|s| s.id == song)
    }

    pub fn current_visit(&self) -> Option<&Visit> {
        self.visits.iter().max_by_key(|v| v.no)
    }

    fn cell(&self, device: Id, song: Id) -> Option<Id> {
        match self.cells.get(&(device, Some(song))) {
            Some(p) => *p,
            None => self.cells.get(&(device, None)).copied().flatten(),
        }
    }

    fn plot_chain(&self, device: Id, song: Id) -> Chain {
        let mut passes: Vec<&Pass> = self
            .passes
            .iter()
            .filter(|p| p.device == device && p.song == song)
            .collect();
        passes.sort_by_key(|p| p.seq);
        let mut holders = vec![self.cell(device, song)];
        holders.extend(passes.iter().map(|p| p.to));
        Chain {
            holders,
            passes: passes.iter().map(|p| p.id).collect(),
            via: None,
        }
    }

    /// Does substitution `s` apply when evaluating `song` as of visit `v`?
    pub fn sub_applies(&self, s: &Substitution, song: Id, v: u32) -> bool {
        if s.state == SubState::Reversed || v < s.from_visit {
            return false;
        }
        if s.ended_at_visit.is_some_and(|e| v >= e) {
            return false;
        }
        if s.range == SubRange::ThisSong {
            let made_on = self.visits.iter().find(|x| x.no == s.from_visit).map(|x| x.song);
            // Its own visit only (a later visit of that song is a fresh start).
            return v == s.from_visit && made_on == Some(song);
        }
        if let Some(start) = s.starts_at_song {
            // Live once any visit since it was made reached its start song or later.
            let start_pos = self.position(start).unwrap_or(0);
            let pos = |sid: Id| self.position(sid).unwrap_or(0);
            let reached = self
                .visits
                .iter()
                .any(|x| x.no >= s.from_visit && x.no <= v && pos(x.song) >= start_pos);
            return reached || pos(song) >= start_pos;
        }
        true
    }

    /// Effective base of a device in a song (TX swaps and retunes applied).
    pub fn effective_base(&self, device: Id, song: Id, v: u32) -> Option<Id> {
        let d = self.device(device)?;
        let mut base = d.base;
        for s in self.subs.iter().filter(|s| self.sub_applies(s, song, v)) {
            match s.kind {
                SubKind::TxSwap if base.is_some() && base == s.failed_base => {
                    base = s.replacement_base
                }
                SubKind::Swap if s.retune && s.replacement == Some(device) => {
                    base = s.failed.and_then(|f| self.device(f)).and_then(|f| f.base);
                    // The failed pack's TX may itself be swapped.
                    for t in self.subs.iter().filter(|t| {
                        t.kind == SubKind::TxSwap && self.sub_applies(t, song, v)
                    }) {
                        if base.is_some() && base == t.failed_base {
                            base = t.replacement_base;
                        }
                    }
                }
                _ => {}
            }
        }
        base
    }

    /// Mix heard on an IEM pack in a song: a TX swap carries the failed TX's mix.
    pub fn mix(&self, device: Id, song: Id, v: u32) -> Option<String> {
        let base = self.effective_base(device, song, v)?;
        for s in self.subs.iter().filter(|s| self.sub_applies(s, song, v)) {
            if s.kind == SubKind::TxSwap && s.replacement_base == Some(base) {
                if let Some(m) = s.failed_base.and_then(|f| self.base_mix.get(&f)) {
                    return Some(m.clone());
                }
            }
        }
        self.base_mix.get(&base).cloned()
    }

    pub fn usability(&self, device: Id, song: Id, v: u32) -> Usability {
        let worst = |a: Usability, b: Usability| {
            use Usability::*;
            let rank = |u: Usability| match u {
                Ok => 0,
                Suspect => 1,
                Out => 2,
                Missing => 3,
            };
            if rank(b) > rank(a) {
                b
            } else {
                a
            }
        };
        let get = |id: Option<Id>| id.and_then(|i| self.equipment.get(&i).copied()).unwrap_or_default();
        let mut u = get(self.carriers.get(&device).copied());
        u = worst(u, get(self.effective_base(device, song, v)));
        u = worst(u, get(self.device(device).and_then(|d| d.element)));
        u
    }

    /// Chains for every device in a song as of visit `v`, plus conflicts the substitutions hit.
    pub fn chains(&self, song: Id, v: u32) -> (BTreeMap<Id, Chain>, Vec<Conflict>) {
        let mut chains: BTreeMap<Id, Chain> = self
            .devices
            .iter()
            .map(|d| (d.id, self.plot_chain(d.id, song)))
            .collect();
        let mut conflicts = Vec::new();
        // Reassigns first: they resolve conflicts the others would otherwise hit (B-3).
        let mut order: Vec<&Substitution> = self
            .subs
            .iter()
            .filter(|s| self.sub_applies(s, song, v))
            .collect();
        order.sort_by_key(|s| (s.kind != SubKind::Reassign, s.id));
        for s in order {
            match s.kind {
                SubKind::TxSwap => {}
                SubKind::Pull => {
                    let Some(f) = s.failed else { continue };
                    let c = chains.get(&f).cloned().unwrap_or_else(Chain::empty);
                    let matches = s.performer.is_none_or(|p| c.mentions(p));
                    // B-5: in the visit the pull was made, the plan doesn't change (no instant
                    // "collect" on stage); from the next visit the device is planned for nobody.
                    if matches && v > s.from_visit {
                        for (k, h) in c.holders.iter().enumerate().skip(1) {
                            if h.is_some() {
                                conflicts.push(Conflict::PassOnPulled {
                                    song,
                                    device: f,
                                    to: c.holders[k],
                                });
                            }
                        }
                        chains.insert(f, Chain { via: Some(s.id), ..Chain::empty() });
                    }
                }
                SubKind::Swap | SubKind::Reassign | SubKind::Guest => {
                    let (Some(f), Some(r)) = (s.failed, s.replacement) else { continue };
                    let c = chains.get(&f).cloned().unwrap_or_else(Chain::empty);
                    let matches = match s.performer {
                        Some(p) => c.mentions(p),
                        None => !c.is_empty(),
                    };
                    if !matches {
                        continue;
                    }
                    // Guest: only that performer's phases move. Others: the whole chain moves.
                    let moved = if s.kind == SubKind::Guest {
                        let p = s.performer;
                        Chain {
                            holders: c.holders.iter().map(|h| if *h == p { *h } else { None }).collect(),
                            passes: c.passes.clone(),
                            via: Some(s.id),
                        }
                    } else {
                        Chain { via: Some(s.id), ..c.clone() }
                    };
                    let target = chains.get(&r).cloned().unwrap_or_else(Chain::empty);
                    if !target.is_empty() {
                        conflicts.push(Conflict::ReplacementTaken {
                            song,
                            sub: s.id,
                            device: r,
                            planned_for: target.holders.iter().flatten().next().copied(),
                        });
                        continue;
                    }
                    let left = if s.kind == SubKind::Guest {
                        Chain {
                            holders: c.holders.iter().map(|h| if *h == s.performer { None } else { *h }).collect(),
                            passes: c.passes.clone(),
                            via: c.via,
                        }
                    } else {
                        Chain { via: Some(s.id), ..Chain::empty() }
                    };
                    chains.insert(r, moved);
                    chains.insert(f, left);
                }
            }
        }
        // TX swaps: one TX can't take two swaps, nor a TX feeding a pack outside the swap.
        let mut taken: BTreeMap<Id, Id> = BTreeMap::new();
        for s in self.subs.iter().filter(|s| s.kind == SubKind::TxSwap && self.sub_applies(s, song, v)) {
            let Some(rb) = s.replacement_base else { continue };
            if taken.insert(rb, s.id).is_some() {
                conflicts.push(Conflict::TxTaken { song, base: rb, sub: s.id });
                continue;
            }
            let other_on_rb = self.devices.iter().any(|d| {
                d.base == Some(rb)
                    && chains.get(&d.id).is_some_and(|c| !c.is_empty())
                    && self.effective_base(d.id, song, v) == Some(rb)
            });
            if other_on_rb {
                conflicts.push(Conflict::TxTaken { song, base: rb, sub: s.id });
            }
        }
        (chains, conflicts)
    }

    /// Phase of a chain in a visit = how many of its passes are marked done in that visit.
    pub fn phase(&self, chain: &Chain, v: u32) -> usize {
        chain
            .passes
            .iter()
            .take_while(|p| self.pass_marks.contains(&(**p, v)))
            .count()
    }

    pub fn planned(&self, device: Id, song: Id, v: u32, phase: Option<usize>) -> Option<Id> {
        let (chains, _) = self.chains(song, v);
        let c = chains.get(&device)?;
        let k = phase.unwrap_or_else(|| self.phase(c, v)).min(c.holders.len() - 1);
        c.holders[k]
    }

    pub fn holding(&self, device: Id) -> Holding {
        self.holdings.get(&device).cloned().unwrap_or_default()
    }

    /// Element planned on a device in a song (plot override, else the default).
    fn planned_element(&self, device: Id, song: Id) -> Option<Id> {
        self.cell_elements
            .get(&(device, Some(song)))
            .or_else(|| self.cell_elements.get(&(device, None)))
            .copied()
            .or_else(|| self.device(device).and_then(|d| d.element))
    }

    pub fn next_song_after(&self, song: Option<Id>) -> Option<Id> {
        let start = match song {
            Some(s) => self.position(s)? + 1,
            None => 0,
        };
        self.songs.iter().skip(start).find(|s| !s.skipped).map(|s| s.id)
    }
}

// ---------------------------------------------------------------- handoffs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// The current song at its current phase: anything here is overdue.
    Now,
    /// The next non-skipped song (or after the show).
    ComingUp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Give { to: Id },
    Collect { from: Id },
    Pass { from: Id, to: Id },
    Capsule { holder: Id, element: Option<Id> },
    /// Sync an IEM pack to another TX (R6 retune, TX swap, or back to its own TX).
    Retune { to_base: Option<Id> },
    /// R4: same performer has it in the next show: nothing to do but leave it.
    Keep { with: Id },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Handoff {
    pub device: Id,
    pub target: Target,
    /// The song the target refers to (None = after the show).
    pub song: Option<Id>,
    pub action: Action,
    /// The device is reached through a substitution (shown as "on spare for Red").
    pub via_sub: Option<Id>,
}

/// A mid-song pass ready to be done in the current visit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PassCard {
    pub pass: Id,
    /// The device it happens on now (the replacement when the original is swapped, B-1).
    pub device: Id,
    pub from: Option<Id>,
    pub to: Option<Id>,
    /// Prep card for the next song (not yet actionable).
    pub prep: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Board {
    pub current_song: Option<Id>,
    pub visit: u32,
    pub now: Vec<Handoff>,
    pub coming_up: Vec<Handoff>,
    pub passes: Vec<PassCard>,
    pub conflicts: Vec<Conflict>,
    /// Devices whose handoffs are owned by a substitution in progress (swap card instead).
    pub held_by_swap: Vec<Id>,
}

impl ShowState {
    /// Devices whose holdings a substitution step owns right now (B-4).
    fn held_by_swap(&self, v: u32) -> BTreeSet<Id> {
        self.subs
            .iter()
            .filter(|s| {
                matches!(s.state, SubState::Recorded | SubState::ReturnRecorded)
                    && s.ended_at_visit.is_none_or(|e| v < e)
            })
            .flat_map(|s| [s.failed, s.replacement])
            .flatten()
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn compare(
        &self,
        device: Id,
        planned: Option<Id>,
        planned_element: Option<Id>,
        planned_base: Option<Id>,
        target: Target,
        song: Option<Id>,
        via_sub: Option<Id>,
        unusable: bool,
    ) -> Vec<Handoff> {
        let h = self.holding(device);
        let mk = |action| Handoff { device, target, song, action, via_sub };
        let mut out = Vec::new();
        let d = self.device(device);
        match (h.holder, planned) {
            (Holder::Owner, _) => return out,
            (Holder::Stock, Some(p)) if !unusable => out.push(mk(Action::Give { to: p })),
            (Holder::Person(a), None) => out.push(mk(Action::Collect { from: a })),
            (Holder::Person(a), Some(b)) if a != b && !unusable => {
                out.push(mk(Action::Pass { from: a, to: b }))
            }
            (Holder::Person(a), Some(b)) if a == b => {
                let want = planned_element;
                let have = h.element.or_else(|| d.and_then(|d| d.element));
                if want != have {
                    out.push(mk(Action::Capsule { holder: a, element: want }));
                }
            }
            _ => {}
        }
        // Pack synced to the wrong TX for what it is about to be used for; and a pack left
        // retuned to another TX goes back to its own once unused (R6).
        if d.is_some_and(|d| d.kind == Kind::Iem) && !unusable {
            let own = d.and_then(|d| d.base);
            let tuned = h.tuned_base.or(own);
            let want = if planned.is_some() { planned_base } else { own };
            if tuned != want {
                out.push(mk(Action::Retune { to_base: want }));
            }
        }
        out
    }

    /// The whole board: overdue (Now), coming up, mid-song passes, conflicts.
    pub fn board(&self) -> Board {
        let Some(cur) = self.current_visit().cloned() else {
            // Pre-show: nothing is overdue; coming up = the first song.
            return self.board_pre_show();
        };
        let v = cur.no;
        let held = self.held_by_swap(v);
        let (chains, mut conflicts) = self.chains(cur.song, v);
        let mut b = Board {
            current_song: Some(cur.song),
            visit: v,
            held_by_swap: held.iter().copied().collect(),
            ..Board::default()
        };

        // Now: current song at the current phase.
        for (dev, c) in &chains {
            if held.contains(dev) {
                continue;
            }
            let k = self.phase(c, v).min(c.holders.len() - 1);
            let unusable = !self.usability(*dev, cur.song, v).usable();
            if self.left.contains(&(v, *dev)) {
                continue;
            }
            b.now.extend(self.compare(
                *dev,
                c.holders[k],
                self.planned_element(*dev, cur.song),
                self.effective_base(*dev, cur.song, v),
                Target::Now,
                Some(cur.song),
                c.via,
                unusable,
            ));
            // Next actionable pass on this chain.
            if k < c.passes.len() {
                b.passes.push(PassCard {
                    pass: c.passes[k],
                    device: *dev,
                    from: c.holders[k],
                    to: c.holders[k + 1],
                    prep: false,
                });
            }
        }
        // The current holder is "who has it after the passes done so far": a Now pass/give that
        // only exists because the next pass isn't done yet is not overdue — compare() already
        // used the current phase, so nothing more to do here.

        // Coming up.
        let next = self.next_song_after(Some(cur.song));
        match next {
            Some(n) => {
                let (nchains, nconf) = self.chains(n, v + 1);
                conflicts.extend(nconf);
                for (dev, c) in &nchains {
                    if held.contains(dev) {
                        continue;
                    }
                    let unusable = !self.usability(*dev, n, v + 1).usable();
                    b.coming_up.extend(self.compare(
                        *dev,
                        c.holders[0],
                        self.planned_element(*dev, n),
                        self.effective_base(*dev, n, v + 1),
                        Target::ComingUp,
                        Some(n),
                        c.via,
                        unusable,
                    ));
                    for (k, p) in c.passes.iter().enumerate() {
                        b.passes.push(PassCard {
                            pass: *p,
                            device: *dev,
                            from: c.holders[k],
                            to: c.holders[k + 1],
                            prep: true,
                        });
                    }
                }
                conflicts.extend(self.song_conflicts(n, v + 1, &nchains));
            }
            None => b.coming_up.extend(self.after_show(&held)),
        }
        conflicts.extend(self.song_conflicts(cur.song, v, &chains));
        // De-duplicate (the same conflict can come from both evaluations).
        let mut seen = Vec::new();
        for c in conflicts {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        b.conflicts = seen;
        b
    }

    fn board_pre_show(&self) -> Board {
        let mut b = Board::default();
        if let Some(first) = self.next_song_after(None) {
            let (chains, conf) = self.chains(first, 1);
            for (dev, c) in &chains {
                let unusable = !self.usability(*dev, first, 1).usable();
                b.coming_up.extend(self.compare(
                    *dev,
                    c.holders[0],
                    self.planned_element(*dev, first),
                    self.effective_base(*dev, first, 1),
                    Target::ComingUp,
                    Some(first),
                    c.via,
                    unusable,
                ));
            }
            b.conflicts = conf;
            b.conflicts.extend(self.song_conflicts(first, 1, &chains));
        }
        b
    }

    /// After the last song: collect everything, except what the same person keeps for the linked
    /// same-day next show (R4).
    fn after_show(&self, held: &BTreeSet<Id>) -> Vec<Handoff> {
        let mut out = Vec::new();
        for d in &self.devices {
            if held.contains(&d.id) {
                continue;
            }
            let h = self.holding(d.id);
            let Holder::Person(p) = h.holder else { continue };
            let keep = self
                .next_show_first
                .as_ref()
                .and_then(|m| m.get(&d.id).copied().flatten())
                == Some(p);
            let action = if keep {
                Action::Keep { with: p }
            } else if d.guest_owner == Some(p) {
                // A guest's own device goes home with them.
                Action::Keep { with: p }
            } else {
                Action::Collect { from: p }
            };
            out.push(Handoff {
                device: d.id,
                target: Target::ComingUp,
                song: None,
                action,
                via_sub: None,
            });
        }
        out
    }

    /// Plot conflicts in one song (two packs, two mics, element twice, unusable, absent).
    pub fn conflicts_for(&self, song: Id, chains: &BTreeMap<Id, Chain>) -> Vec<Conflict> {
        let v = self.current_visit().map(|x| x.no).unwrap_or(1);
        self.song_conflicts(song, v, chains)
    }

    fn song_conflicts(&self, song: Id, v: u32, chains: &BTreeMap<Id, Chain>) -> Vec<Conflict> {
        let mut out = Vec::new();
        let phases = chains.values().map(|c| c.holders.len()).max().unwrap_or(1);
        for k in 0..phases {
            let at = |c: &Chain| c.holders[k.min(c.holders.len() - 1)];
            let mut by_person: BTreeMap<(Id, Kind), Vec<Id>> = BTreeMap::new();
            let mut by_element: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
            for (dev, c) in chains {
                let Some(p) = at(c) else { continue };
                let Some(d) = self.device(*dev) else { continue };
                by_person.entry((p, d.kind)).or_default().push(*dev);
                if let Some(e) = self.planned_element(*dev, song) {
                    by_element.entry(e).or_default().push(*dev);
                }
            }
            for ((p, kind), devs) in by_person {
                if devs.len() < 2 {
                    continue;
                }
                out.push(match kind {
                    Kind::Mic => Conflict::TwoMics { song, performer: p, devices: devs },
                    _ => Conflict::TwoPacks { song, performer: p, kind, devices: devs },
                });
            }
            for (e, devs) in by_element {
                if devs.len() > 1 {
                    out.push(Conflict::ElementTwice { song, element: e, devices: devs });
                }
            }
        }
        for (dev, c) in chains {
            for p in c.holders.iter().flatten() {
                if !self.usability(*dev, song, v).usable() {
                    out.push(Conflict::UnusablePlanned { song, device: *dev, performer: *p });
                }
                if self.absent.contains(p) {
                    out.push(Conflict::AbsentPlanned { song, device: *dev, performer: *p });
                }
            }
        }
        let mut seen = Vec::new();
        for c in out {
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
        seen
    }
}

// ---------------------------------------------------------------- operations (state changes)

/// Holding changes produced by an operation; the server writes them in one transaction.
pub type Moves = Vec<(Id, Holding)>;

impl ShowState {
    /// Done on a handoff (R8: one tap).
    pub fn do_handoff(&self, h: &Handoff) -> Moves {
        let mut cur = self.holding(h.device);
        match h.action {
            Action::Give { to } | Action::Pass { to, .. } => cur.holder = Holder::Person(to),
            Action::Collect { .. } => {
                cur.holder = Holder::Stock;
                cur.element = None;
            }
            Action::Capsule { element, .. } => cur.element = element,
            Action::Retune { to_base } => {
                let own = self.device(h.device).and_then(|d| d.base);
                cur.tuned_base = if to_base == own { None } else { to_base };
            }
            Action::Keep { .. } => return vec![],
        }
        vec![(h.device, cur)]
    }

    /// Pass done in the current visit: marks the pass and moves the holding together (FIX 5).
    pub fn pass_done(&self, card: &PassCard) -> (Moves, (Id, u32)) {
        let v = self.current_visit().map(|v| v.no).unwrap_or(0);
        let mut h = self.holding(card.device);
        h.holder = card.to.map(Holder::Person).unwrap_or(Holder::Stock);
        (vec![(card.device, h)], (card.pass, v))
    }

    /// In hand for a swap / reassign / guest (B-4, FIX 6): the replacement comes from wherever it
    /// is to the performer; the failed device goes to stock only if the performer had it.
    pub fn sub_in_hand(&self, s: &Substitution, performer: Id) -> Moves {
        let mut out = Vec::new();
        if let Some(r) = s.replacement {
            let mut h = self.holding(r);
            h.holder = Holder::Person(performer);
            if s.retune {
                h.tuned_base = s.failed.and_then(|f| self.device(f)).and_then(|f| f.base);
            }
            out.push((r, h));
        }
        if let Some(f) = s.failed {
            let h = self.holding(f);
            if h.holder == Holder::Person(performer) && s.kind != SubKind::Pull {
                out.push((f, Holding { holder: Holder::Stock, ..h }));
            }
        }
        out
    }

    /// Return in hand: the original goes back to the performer, and whatever they actually hold
    /// in its place goes to stock.
    pub fn return_in_hand(&self, s: &Substitution, performer: Id) -> Moves {
        let mut out = Vec::new();
        if let Some(f) = s.failed {
            let mut h = self.holding(f);
            h.holder = Holder::Person(performer);
            out.push((f, h));
        }
        if let Some(r) = s.replacement {
            let h = self.holding(r);
            if h.holder == Holder::Person(performer) {
                out.push((r, Holding { holder: Holder::Stock, ..h }));
            }
        }
        out
    }
}

// ---------------------------------------------------------------- swap candidates

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "tier", rename_all = "snake_case")]
pub enum CandidateTier {
    /// A designated spare, free.
    Spare,
    /// Not used now; free until that song (it is planned for someone there).
    FreeUntil { song: Id, by: Id },
    /// Not used again in this show.
    FreeRestOfShow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub device: Id,
    pub tier: CandidateTier,
    /// IEM on another TX: sync it to the failed pack's TX first (R6).
    pub retune: bool,
    /// Same receiver unit as the failed mic, or another kind of mic: shown, flagged.
    pub flag: Option<String>,
}

impl ShowState {
    /// Devices that could replace `failed` now, best first: spares, then free devices, of the
    /// same kind, usable, not held by anyone else, not in another swap.
    pub fn swap_candidates(&self, failed: Id) -> Vec<Candidate> {
        let Some(f) = self.device(failed) else { return vec![] };
        let (song, v) = match self.current_visit() {
            Some(c) => (c.song, c.no),
            None => match self.next_song_after(None) {
                Some(s) => (s, 0),
                None => return vec![],
            },
        };
        let (chains, _) = self.chains(song, v);
        let in_use = |d: Id| chains.get(&d).is_some_and(|c| !c.is_empty());
        let busy_in_sub: BTreeSet<Id> = self
            .subs
            .iter()
            .filter(|s| self.sub_applies(s, song, v) && s.state != SubState::Reversed)
            .flat_map(|s| [s.replacement])
            .flatten()
            .collect();
        let mut out = Vec::new();
        for d in &self.devices {
            if d.id == failed || d.kind != f.kind || in_use(d.id) || busy_in_sub.contains(&d.id) {
                continue;
            }
            if d.guest_owner.is_some() || !self.usability(d.id, song, v).usable() {
                continue;
            }
            // Held by someone (not yet collected) → not free.
            if matches!(self.holding(d.id).holder, Holder::Person(_) | Holder::Owner) {
                continue;
            }
            // When is it next needed?
            let start = self.position(song).map(|p| p + 1).unwrap_or(0);
            let next_use = self.songs.iter().skip(start).filter(|s| !s.skipped).find_map(|s| {
                self.planned(d.id, s.id, v + 1, Some(0)).map(|p| (s.id, p))
            });
            let tier = if d.is_spare && next_use.is_none() {
                CandidateTier::Spare
            } else if let Some((s, p)) = next_use {
                CandidateTier::FreeUntil { song: s, by: p }
            } else {
                CandidateTier::FreeRestOfShow
            };
            let retune = f.kind == Kind::Iem && d.base != f.base;
            let flag = (f.kind == Kind::Mic && d.base.is_some() && f.base.is_some()
                && self.base_parent(d.base) == self.base_parent(f.base))
                .then(|| "same receiver as the failed mic".to_string());
            out.push(Candidate { device: d.id, tier, retune, flag });
        }
        let rank = |c: &Candidate| match c.tier {
            CandidateTier::Spare => 0,
            CandidateTier::FreeRestOfShow => 1,
            CandidateTier::FreeUntil { .. } => 2,
        };
        out.sort_by_key(|c| (rank(c), c.retune, c.flag.is_some(), c.device));
        out
    }

    /// Receiver unit of a receiver channel, when known (`base_parents`).
    fn base_parent(&self, base: Option<Id>) -> Option<Id> {
        base.and_then(|b| self.base_parents.get(&b).copied())
    }
}

/// R5: a battery check counts for a show only if made at or after
/// max(call − 2 h (or activation), the end of the linked previous show).
pub fn battery_evidence_start(
    call_at: Option<chrono::DateTime<chrono::Utc>>,
    activated_at: Option<chrono::DateTime<chrono::Utc>>,
    prev_ended_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let base = call_at
        .map(|c| c - chrono::Duration::hours(2))
        .or(activated_at);
    match (base, prev_ended_at) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

// ---------------------------------------------------------------- usage (tile state)

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "usage", rename_all = "snake_case")]
pub enum Usage {
    Out,
    Suspect,
    Missing,
    Pulled,
    OnSpareFor { failed: Id },
    InUse { by: Id },
    Collect { from: Id },
    Next { song: Id, by: Id },
    Spare,
    Done,
}

impl ShowState {
    pub fn usage(&self, device: Id) -> Usage {
        let Some(cur) = self.current_visit().cloned() else {
            return self.usage_at(device, None, 0);
        };
        self.usage_at(device, Some(cur.song), cur.no)
    }

    fn usage_at(&self, device: Id, song: Option<Id>, v: u32) -> Usage {
        let u = match song {
            Some(s) => self.usability(device, s, v),
            None => self
                .carriers
                .get(&device)
                .and_then(|c| self.equipment.get(c).copied())
                .unwrap_or_default(),
        };
        match u {
            Usability::Out => return Usage::Out,
            Usability::Missing => return Usage::Missing,
            Usability::Suspect => {}
            Usability::Ok => {}
        }
        let live = |s: &&Substitution| song.is_some_and(|sg| self.sub_applies(s, sg, v));
        if self
            .subs
            .iter()
            .filter(live)
            .any(|s| s.kind == SubKind::Pull && s.failed == Some(device))
        {
            return Usage::Pulled;
        }
        if u == Usability::Suspect {
            return Usage::Suspect;
        }
        if let Some(s) = self.subs.iter().filter(live).find(|s| {
            matches!(s.kind, SubKind::Swap | SubKind::Guest) && s.replacement == Some(device)
        }) {
            if let Some(f) = s.failed {
                return Usage::OnSpareFor { failed: f };
            }
        }
        if let Some(sg) = song {
            if let Some(p) = self.planned(device, sg, v, None) {
                return Usage::InUse { by: p };
            }
        }
        // Later songs.
        let start = song.and_then(|s| self.position(s)).map(|p| p + 1).unwrap_or(0);
        for s in self.songs.iter().skip(start).filter(|s| !s.skipped) {
            if let Some(p) = self.planned(device, s.id, v + 1, Some(0)) {
                if let Holder::Person(h) = self.holding(device).holder {
                    if h != p {
                        return Usage::Collect { from: h };
                    }
                }
                return Usage::Next { song: s.id, by: p };
            }
        }
        if let Holder::Person(h) = self.holding(device).holder {
            return Usage::Collect { from: h };
        }
        if self.device(device).is_some_and(|d| d.is_spare) {
            return Usage::Spare;
        }
        Usage::Done
    }

    /// Count-in: devices not back in stock, except ones kept for the next show (R4) or gone
    /// home with their guest owner.
    pub fn count_in(&self) -> Vec<(Id, Holder)> {
        self.devices
            .iter()
            .filter_map(|d| {
                let h = self.holding(d.id).holder;
                let Holder::Person(p) = h else { return None };
                let kept = self
                    .next_show_first
                    .as_ref()
                    .and_then(|m| m.get(&d.id).copied().flatten())
                    == Some(p);
                (!kept).then_some((d.id, h))
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
