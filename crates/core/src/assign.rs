//! Assignment overlay, conflict detection and handoff computation.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::model::{Assignment, EquipmentStatus, Id, Kit, KitKind, Song};

/// performer id -> sorted, de-duplicated kit ids in effect.
pub type Effective = BTreeMap<Id, Vec<Id>>;

/// Resolve which kits each performer holds in `song_id` (or the show defaults when `None`).
pub fn effective_assignment(assignments: &[Assignment], song_id: Option<Id>) -> Effective {
    let mut defaults: BTreeMap<(Id, KitKind), Vec<Option<Id>>> = BTreeMap::new();
    let mut overrides: BTreeMap<(Id, KitKind), Vec<Option<Id>>> = BTreeMap::new();
    for a in assignments {
        let key = (a.performer_id, a.kind);
        match a.song_id {
            None => defaults.entry(key).or_default().push(a.kit_id),
            Some(s) if Some(s) == song_id => overrides.entry(key).or_default().push(a.kit_id),
            Some(_) => {}
        }
    }

    let keys: BTreeSet<(Id, KitKind)> = defaults.keys().chain(overrides.keys()).copied().collect();
    let mut out: Effective = BTreeMap::new();
    for key in keys {
        let rows = overrides.get(&key).or_else(|| defaults.get(&key));
        let kits = rows.into_iter().flatten().flatten().copied();
        let entry = out.entry(key.0).or_default();
        entry.extend(kits);
    }
    for kits in out.values_mut() {
        kits.sort_unstable();
        kits.dedup();
    }
    out.retain(|_, kits| !kits.is_empty());
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Conflict {
    /// One physical item is held by more than one performer in the same song.
    ItemShared {
        item_id: Id,
        performer_ids: Vec<Id>,
        kit_ids: Vec<Id>,
    },
    /// An assignment points at a kit that does not exist.
    UnknownKit { performer_id: Id, kit_id: Id },
    /// Assignment kind differs from the kit kind.
    KindMismatch {
        assignment_id: Id,
        kit_id: Id,
        expected: KitKind,
        actual: KitKind,
    },
    /// A kit contains an item that is faulty or retired.
    UnusableItem {
        item_id: Id,
        kit_id: Id,
        performer_id: Id,
        status: EquipmentStatus,
    },
}

/// Conflicts in a single song (or in the defaults when `song_id` is `None`).
pub fn conflicts_for(
    assignments: &[Assignment],
    kits: &HashMap<Id, Kit>,
    statuses: &HashMap<Id, EquipmentStatus>,
    song_id: Option<Id>,
) -> Vec<Conflict> {
    let mut out = Vec::new();
    let eff = effective_assignment(assignments, song_id);

    // item -> (performer, kit)
    let mut holders: BTreeMap<Id, BTreeSet<(Id, Id)>> = BTreeMap::new();
    for (&performer, kit_ids) in &eff {
        for &kit_id in kit_ids {
            let Some(kit) = kits.get(&kit_id) else {
                out.push(Conflict::UnknownKit {
                    performer_id: performer,
                    kit_id,
                });
                continue;
            };
            for item in kit.items() {
                holders.entry(item).or_default().insert((performer, kit_id));
                if let Some(&status) = statuses.get(&item) {
                    if status.is_unusable() {
                        out.push(Conflict::UnusableItem {
                            item_id: item,
                            kit_id,
                            performer_id: performer,
                            status,
                        });
                    }
                }
            }
        }
    }
    for (item_id, set) in holders {
        let performer_ids: BTreeSet<Id> = set.iter().map(|(p, _)| *p).collect();
        if performer_ids.len() > 1 {
            let kit_ids: BTreeSet<Id> = set.iter().map(|(_, k)| *k).collect();
            out.push(Conflict::ItemShared {
                item_id,
                performer_ids: performer_ids.into_iter().collect(),
                kit_ids: kit_ids.into_iter().collect(),
            });
        }
    }
    out
}

/// Kind mismatches are static (independent of song) so they are checked once.
pub fn kind_mismatches(assignments: &[Assignment], kits: &HashMap<Id, Kit>) -> Vec<Conflict> {
    assignments
        .iter()
        .filter_map(|a| {
            let kit = kits.get(&a.kit_id?)?;
            (kit.kind != a.kind).then_some(Conflict::KindMismatch {
                assignment_id: a.id,
                kit_id: kit.id,
                expected: a.kind,
                actual: kit.kind,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Handoff {
    /// Item moves from one performer to another.
    Transfer {
        item_id: Id,
        from_performer: Id,
        to_performer: Id,
        to_kit: Id,
    },
    /// Item goes from stock/rack to a performer.
    Issue {
        item_id: Id,
        to_performer: Id,
        to_kit: Id,
    },
    /// Item comes back from a performer to stock/rack.
    Return { item_id: Id, from_performer: Id },
    /// The element (capsule/headset/lav) on a carrier changes between songs.
    ComponentChange {
        carrier_id: Id,
        from_element: Option<Id>,
        to_element: Option<Id>,
        performer_id: Id,
    },
}

impl Handoff {
    /// Performers who must act on this handoff.
    pub fn performers(&self) -> Vec<Id> {
        match *self {
            Handoff::Transfer {
                from_performer,
                to_performer,
                ..
            } => vec![from_performer, to_performer],
            Handoff::Issue { to_performer, .. } => vec![to_performer],
            Handoff::Return { from_performer, .. } => vec![from_performer],
            Handoff::ComponentChange { performer_id, .. } => vec![performer_id],
        }
    }
}

/// portable item -> (performer, kit). On conflicts the lowest performer id wins; conflicts are
/// reported separately by [`conflicts_for`].
fn portable_holders(eff: &Effective, kits: &HashMap<Id, Kit>) -> BTreeMap<Id, (Id, Id)> {
    let mut out = BTreeMap::new();
    for (&performer, kit_ids) in eff {
        for &kit_id in kit_ids {
            if let Some(kit) = kits.get(&kit_id) {
                for item in kit.portable_items() {
                    out.entry(item).or_insert((performer, kit_id));
                }
            }
        }
    }
    out
}

/// carrier -> (performer, element) for carriers in use.
fn carrier_elements(eff: &Effective, kits: &HashMap<Id, Kit>) -> BTreeMap<Id, (Id, Option<Id>)> {
    let mut out = BTreeMap::new();
    for (&performer, kit_ids) in eff {
        for &kit_id in kit_ids {
            if let Some(kit) = kits.get(&kit_id) {
                if let Some(c) = kit.carrier_id {
                    out.entry(c).or_insert((performer, kit.element_id));
                }
            }
        }
    }
    out
}

/// Handoffs needed to go from `before` to `after`.
pub fn handoffs_between(
    before: &Effective,
    after: &Effective,
    kits: &HashMap<Id, Kit>,
) -> Vec<Handoff> {
    let hb = portable_holders(before, kits);
    let ha = portable_holders(after, kits);
    let items: BTreeSet<Id> = hb.keys().chain(ha.keys()).copied().collect();
    let mut out = Vec::new();
    for item in items {
        match (hb.get(&item), ha.get(&item)) {
            (Some(&(p1, _)), Some(&(p2, k2))) if p1 != p2 => out.push(Handoff::Transfer {
                item_id: item,
                from_performer: p1,
                to_performer: p2,
                to_kit: k2,
            }),
            (None, Some(&(p2, k2))) => out.push(Handoff::Issue {
                item_id: item,
                to_performer: p2,
                to_kit: k2,
            }),
            (Some(&(p1, _)), None) => out.push(Handoff::Return {
                item_id: item,
                from_performer: p1,
            }),
            _ => {}
        }
    }

    let cb = carrier_elements(before, kits);
    let ca = carrier_elements(after, kits);
    for (carrier, &(performer, to_el)) in &ca {
        if let Some(&(_, from_el)) = cb.get(carrier) {
            if from_el != to_el {
                out.push(Handoff::ComponentChange {
                    carrier_id: *carrier,
                    from_element: from_el,
                    to_element: to_el,
                    performer_id: performer,
                });
            }
        }
    }
    out.sort();
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    /// `None` = before the show starts.
    pub from_song: Option<Id>,
    /// `None` = after the show ends.
    pub to_song: Option<Id>,
    pub handoffs: Vec<Handoff>,
}

/// Full handoff plan for a show: pre-show issue, every song-to-song transition, post-show return.
/// Transitions with no handoffs are omitted.
pub fn handoff_plan(
    songs: &[Song],
    assignments: &[Assignment],
    kits: &HashMap<Id, Kit>,
) -> Vec<Transition> {
    let mut ordered: Vec<&Song> = songs.iter().collect();
    ordered.sort_by_key(|s| (s.position, s.id));

    let empty = Effective::new();
    let mut prev: (Option<Id>, Effective) = (None, empty.clone());
    let mut out = Vec::new();
    for song in ordered {
        let eff = effective_assignment(assignments, Some(song.id));
        let h = handoffs_between(&prev.1, &eff, kits);
        if !h.is_empty() {
            out.push(Transition {
                from_song: prev.0,
                to_song: Some(song.id),
                handoffs: h,
            });
        }
        prev = (Some(song.id), eff);
    }
    if prev.0.is_some() {
        let h = handoffs_between(&prev.1, &empty, kits);
        if !h.is_empty() {
            out.push(Transition {
                from_song: prev.0,
                to_song: None,
                handoffs: h,
            });
        }
    }
    out
}

/// The next `count` transitions after the current song (the transition into the current song
/// is not included). With `current == None`, starts with the pre-show transition.
pub fn upcoming_transitions(
    plan: &[Transition],
    songs: &[Song],
    current: Option<Id>,
    count: usize,
) -> Vec<Transition> {
    let mut ordered: Vec<&Song> = songs.iter().collect();
    ordered.sort_by_key(|s| (s.position, s.id));
    let start_idx = match current {
        None => None,
        Some(c) => ordered.iter().position(|s| s.id == c),
    };
    // Songs after the current one, in order; "from" of a relevant transition must be current or later.
    let relevant_from: BTreeSet<Option<Id>> = match start_idx {
        None => std::iter::once(None)
            .chain(ordered.iter().map(|s| Some(s.id)))
            .collect(),
        Some(i) => ordered[i..].iter().map(|s| Some(s.id)).collect(),
    };
    plan.iter()
        .filter(|t| relevant_from.contains(&t.from_song))
        .take(count)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kit(
        id: Id,
        kind: KitKind,
        base: Option<Id>,
        carrier: Option<Id>,
        element: Option<Id>,
    ) -> Kit {
        Kit {
            id,
            kind,
            name: format!("kit{id}"),
            base_id: base,
            carrier_id: carrier,
            element_id: element,
            console_channel: None,
            console_channel_name: None,
            dca: None,
            iem_mix: None,
            is_spare: false,
        }
    }

    fn mic(id: Id, rx: Id, tx: Id, cap: Option<Id>) -> Kit {
        kit(id, KitKind::Mic, Some(rx), Some(tx), cap)
    }

    fn kits(list: Vec<Kit>) -> HashMap<Id, Kit> {
        list.into_iter().map(|k| (k.id, k)).collect()
    }

    struct A(Vec<Assignment>);
    impl A {
        fn new() -> Self {
            A(Vec::new())
        }
        fn add(mut self, performer: Id, kind: KitKind, kit: Option<Id>, song: Option<Id>) -> Self {
            let id = self.0.len() as Id + 1;
            self.0.push(Assignment {
                id,
                performer_id: performer,
                kind,
                kit_id: kit,
                song_id: song,
            });
            self
        }
        fn def(self, p: Id, k: Id) -> Self {
            self.add(p, KitKind::Mic, Some(k), None)
        }
        fn ovr(self, p: Id, k: Option<Id>, s: Id) -> Self {
            self.add(p, KitKind::Mic, k, Some(s))
        }
    }

    fn songs(n: i64) -> Vec<Song> {
        (1..=n)
            .map(|i| Song {
                id: 100 + i,
                position: i,
                title: format!("S{i}"),
            })
            .collect()
    }

    // ---------- effective assignment ----------

    #[test]
    fn defaults_apply_to_every_song() {
        let a = A::new().def(1, 10).def(2, 11).0;
        let e = effective_assignment(&a, Some(101));
        assert_eq!(e[&1], vec![10]);
        assert_eq!(e[&2], vec![11]);
        assert_eq!(effective_assignment(&a, None), e);
    }

    #[test]
    fn override_replaces_default_of_same_kind_only_in_that_song() {
        let a = A::new().def(1, 10).ovr(1, Some(12), 102).0;
        assert_eq!(effective_assignment(&a, Some(101))[&1], vec![10]);
        assert_eq!(effective_assignment(&a, Some(102))[&1], vec![12]);
        assert_eq!(effective_assignment(&a, Some(103))[&1], vec![10]);
    }

    #[test]
    fn override_does_not_touch_other_kinds() {
        let a = A::new()
            .def(1, 10)
            .add(1, KitKind::Iem, Some(20), None)
            .ovr(1, Some(12), 102)
            .0;
        assert_eq!(effective_assignment(&a, Some(102))[&1], vec![12, 20]);
    }

    #[test]
    fn none_override_removes_kind_for_song() {
        let a = A::new().def(1, 10).ovr(1, None, 102).0;
        assert!(!effective_assignment(&a, Some(102)).contains_key(&1));
        assert_eq!(effective_assignment(&a, Some(101))[&1], vec![10]);
    }

    #[test]
    fn override_without_default_adds_kit() {
        let a = A::new().ovr(3, Some(14), 102).0;
        assert!(!effective_assignment(&a, Some(101)).contains_key(&3));
        assert_eq!(effective_assignment(&a, Some(102))[&3], vec![14]);
    }

    #[test]
    fn multiple_kits_of_same_kind_allowed_and_deduped() {
        let a = A::new().def(1, 10).def(1, 11).def(1, 10).0;
        assert_eq!(effective_assignment(&a, Some(101))[&1], vec![10, 11]);
    }

    #[test]
    fn multiple_override_rows_replace_all_defaults() {
        let a = A::new()
            .def(1, 10)
            .def(1, 11)
            .ovr(1, Some(12), 102)
            .ovr(1, Some(13), 102)
            .0;
        assert_eq!(effective_assignment(&a, Some(102))[&1], vec![12, 13]);
    }

    // ---------- conflicts ----------

    #[test]
    fn no_conflicts_for_clean_plan() {
        let k = kits(vec![
            mic(10, 1000, 2000, Some(3000)),
            mic(11, 1001, 2001, Some(3001)),
        ]);
        let a = A::new().def(1, 10).def(2, 11).0;
        assert!(conflicts_for(&a, &k, &HashMap::new(), Some(101)).is_empty());
    }

    #[test]
    fn same_kit_to_two_performers_is_conflict() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new().def(1, 10).def(2, 10).0;
        let c = conflicts_for(&a, &k, &HashMap::new(), Some(101));
        assert_eq!(c.len(), 2); // receiver channel and transmitter both shared
        assert!(c.iter().all(|c| matches!(c, Conflict::ItemShared { performer_ids, .. } if performer_ids == &vec![1, 2])));
    }

    #[test]
    fn shared_transmitter_across_kits_is_conflict() {
        let k = kits(vec![mic(10, 1000, 2000, None), mic(11, 1001, 2000, None)]);
        let a = A::new().def(1, 10).def(2, 11).0;
        let c = conflicts_for(&a, &k, &HashMap::new(), Some(101));
        assert_eq!(
            c,
            vec![Conflict::ItemShared {
                item_id: 2000,
                performer_ids: vec![1, 2],
                kit_ids: vec![10, 11]
            }]
        );
    }

    #[test]
    fn forgetting_to_release_default_creates_conflict_in_override_song() {
        // B borrows A's kit in song 2 but A was not overridden to None.
        let k = kits(vec![mic(10, 1000, 2000, None), mic(11, 1001, 2001, None)]);
        let a = A::new().def(1, 10).def(2, 11).ovr(2, Some(10), 102).0;
        assert!(conflicts_for(&a, &k, &HashMap::new(), Some(101)).is_empty());
        assert!(!conflicts_for(&a, &k, &HashMap::new(), Some(102)).is_empty());
        let fixed = A::new()
            .def(1, 10)
            .def(2, 11)
            .ovr(2, Some(10), 102)
            .ovr(1, None, 102)
            .0;
        assert!(conflicts_for(&fixed, &k, &HashMap::new(), Some(102)).is_empty());
    }

    #[test]
    fn unknown_kit_reported() {
        let a = A::new().def(1, 99).0;
        assert_eq!(
            conflicts_for(&a, &HashMap::new(), &HashMap::new(), None),
            vec![Conflict::UnknownKit {
                performer_id: 1,
                kit_id: 99
            }]
        );
    }

    #[test]
    fn faulty_item_reported() {
        let k = kits(vec![mic(10, 1000, 2000, Some(3000))]);
        let a = A::new().def(1, 10).0;
        let st = HashMap::from([
            (3000, EquipmentStatus::Faulty),
            (2000, EquipmentStatus::InUse),
        ]);
        assert_eq!(
            conflicts_for(&a, &k, &st, None),
            vec![Conflict::UnusableItem {
                item_id: 3000,
                kit_id: 10,
                performer_id: 1,
                status: EquipmentStatus::Faulty
            }]
        );
    }

    #[test]
    fn kind_mismatch_reported() {
        let k = kits(vec![kit(20, KitKind::Iem, Some(1), Some(2), None)]);
        let a = A::new().def(1, 20).0; // assigned as Mic
        assert_eq!(
            kind_mismatches(&a, &k),
            vec![Conflict::KindMismatch {
                assignment_id: 1,
                kit_id: 20,
                expected: KitKind::Mic,
                actual: KitKind::Iem
            }]
        );
    }

    // ---------- handoffs ----------

    #[test]
    fn static_show_only_has_issue_and_return() {
        let k = kits(vec![mic(10, 1000, 2000, Some(3000))]);
        let a = A::new().def(1, 10).0;
        let plan = handoff_plan(&songs(3), &a, &k);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].from_song, None);
        assert_eq!(plan[0].to_song, Some(101));
        assert_eq!(
            plan[0].handoffs,
            vec![
                Handoff::Issue {
                    item_id: 2000,
                    to_performer: 1,
                    to_kit: 10
                },
                Handoff::Issue {
                    item_id: 3000,
                    to_performer: 1,
                    to_kit: 10
                },
            ]
        );
        assert_eq!(plan[1].from_song, Some(103));
        assert_eq!(plan[1].to_song, None);
        assert!(plan[1].handoffs.iter().all(|h| matches!(
            h,
            Handoff::Return {
                from_performer: 1,
                ..
            }
        )));
    }

    #[test]
    fn receiver_channel_never_handed_off() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new().def(1, 10).0;
        let plan = handoff_plan(&songs(1), &a, &k);
        assert!(plan.iter().flat_map(|t| &t.handoffs).all(|h| match h {
            Handoff::Issue { item_id, .. } | Handoff::Return { item_id, .. } => *item_id != 1000,
            _ => true,
        }));
    }

    #[test]
    fn whole_kit_transfer_between_performers() {
        let k = kits(vec![mic(10, 1000, 2000, Some(3000))]);
        let a = A::new()
            .def(1, 10)
            .ovr(1, None, 102)
            .ovr(2, Some(10), 102)
            .0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert_eq!(
            t.handoffs,
            vec![
                Handoff::Transfer {
                    item_id: 2000,
                    from_performer: 1,
                    to_performer: 2,
                    to_kit: 10
                },
                Handoff::Transfer {
                    item_id: 3000,
                    from_performer: 1,
                    to_performer: 2,
                    to_kit: 10
                },
            ]
        );
    }

    #[test]
    fn transfer_back_after_one_song() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new()
            .def(1, 10)
            .ovr(1, None, 102)
            .ovr(2, Some(10), 102)
            .0;
        let plan = handoff_plan(&songs(3), &a, &k);
        let back = plan.iter().find(|t| t.to_song == Some(103)).unwrap();
        assert_eq!(
            back.handoffs,
            vec![Handoff::Transfer {
                item_id: 2000,
                from_performer: 2,
                to_performer: 1,
                to_kit: 10
            }]
        );
    }

    #[test]
    fn capsule_swap_on_same_transmitter_same_performer() {
        // Kit 10: TX 2000 with SM58 capsule 3000; Kit 11: same TX & RX with Beta87 capsule 3001.
        let k = kits(vec![
            mic(10, 1000, 2000, Some(3000)),
            mic(11, 1000, 2000, Some(3001)),
        ]);
        let a = A::new().def(1, 10).ovr(1, Some(11), 102).0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert_eq!(
            t.handoffs,
            vec![
                Handoff::Issue {
                    item_id: 3001,
                    to_performer: 1,
                    to_kit: 11
                },
                Handoff::Return {
                    item_id: 3000,
                    from_performer: 1
                },
                Handoff::ComponentChange {
                    carrier_id: 2000,
                    from_element: Some(3000),
                    to_element: Some(3001),
                    performer_id: 1
                },
            ]
        );
    }

    #[test]
    fn capsule_moves_between_transmitters() {
        // Capsule 3000 moves from performer 1's TX to performer 2's TX.
        let k = kits(vec![
            mic(10, 1000, 2000, Some(3000)),
            mic(11, 1001, 2001, Some(3001)),
            mic(12, 1000, 2000, None),
            mic(13, 1001, 2001, Some(3000)),
        ]);
        let a = A::new()
            .def(1, 10)
            .def(2, 11)
            .ovr(1, Some(12), 102)
            .ovr(2, Some(13), 102)
            .0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert!(t.handoffs.contains(&Handoff::Transfer {
            item_id: 3000,
            from_performer: 1,
            to_performer: 2,
            to_kit: 13
        }));
        assert!(t.handoffs.contains(&Handoff::Return {
            item_id: 3001,
            from_performer: 2
        }));
        assert!(t.handoffs.contains(&Handoff::ComponentChange {
            carrier_id: 2000,
            from_element: Some(3000),
            to_element: None,
            performer_id: 1
        }));
        assert!(t.handoffs.contains(&Handoff::ComponentChange {
            carrier_id: 2001,
            from_element: Some(3001),
            to_element: Some(3000),
            performer_id: 2
        }));
    }

    #[test]
    fn headset_change_on_bodypack() {
        let k = kits(vec![
            mic(10, 1000, 2000, Some(4000)),
            mic(11, 1000, 2000, Some(4001)),
        ]);
        let a = A::new().def(1, 10).ovr(1, Some(11), 102).0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert!(t.handoffs.iter().any(|h| matches!(
            h,
            Handoff::ComponentChange {
                carrier_id: 2000,
                ..
            }
        )));
    }

    #[test]
    fn performer_absent_from_middle_song_returns_and_reissues() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new().def(1, 10).ovr(1, None, 102).0;
        let plan = handoff_plan(&songs(3), &a, &k);
        let into2 = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert_eq!(
            into2.handoffs,
            vec![Handoff::Return {
                item_id: 2000,
                from_performer: 1
            }]
        );
        let into3 = plan.iter().find(|t| t.to_song == Some(103)).unwrap();
        assert_eq!(
            into3.handoffs,
            vec![Handoff::Issue {
                item_id: 2000,
                to_performer: 1,
                to_kit: 10
            }]
        );
    }

    #[test]
    fn performer_only_in_last_song() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new().ovr(5, Some(10), 103).0;
        let plan = handoff_plan(&songs(3), &a, &k);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].from_song, Some(102));
        assert_eq!(plan[0].to_song, Some(103));
        assert_eq!(plan[1].to_song, None);
    }

    #[test]
    fn iem_pack_transfer() {
        let k = kits(vec![kit(20, KitKind::Iem, Some(5000), Some(6000), None)]);
        let a = A::new()
            .add(1, KitKind::Iem, Some(20), None)
            .add(1, KitKind::Iem, None, Some(102))
            .add(2, KitKind::Iem, Some(20), Some(102))
            .0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert_eq!(
            t.handoffs,
            vec![Handoff::Transfer {
                item_id: 6000,
                from_performer: 1,
                to_performer: 2,
                to_kit: 20
            }]
        );
    }

    #[test]
    fn three_way_rotation() {
        // 1->2, 2->3, 3->1 between song 1 and 2.
        let k = kits(vec![
            mic(10, 1000, 2000, None),
            mic(11, 1001, 2001, None),
            mic(12, 1002, 2002, None),
        ]);
        let a = A::new()
            .def(1, 10)
            .def(2, 11)
            .def(3, 12)
            .ovr(1, Some(12), 102)
            .ovr(2, Some(10), 102)
            .ovr(3, Some(11), 102)
            .0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert_eq!(t.handoffs.len(), 3);
        assert!(t
            .handoffs
            .iter()
            .all(|h| matches!(h, Handoff::Transfer { .. })));
        assert!(conflicts_for(&a, &k, &HashMap::new(), Some(102)).is_empty());
    }

    #[test]
    fn song_order_uses_position_not_id() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let s = vec![
            Song {
                id: 1,
                position: 2,
                title: "B".into(),
            },
            Song {
                id: 2,
                position: 1,
                title: "A".into(),
            },
        ];
        let a = A::new().ovr(1, Some(10), 1).0; // only in song id 1 which is second
        let plan = handoff_plan(&s, &a, &k);
        assert_eq!(plan[0].from_song, Some(2));
        assert_eq!(plan[0].to_song, Some(1));
    }

    #[test]
    fn unchanged_transitions_omitted() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new().def(1, 10).0;
        let plan = handoff_plan(&songs(5), &a, &k);
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn empty_show_has_no_plan() {
        assert!(handoff_plan(&[], &[], &HashMap::new()).is_empty());
        assert!(handoff_plan(&songs(3), &[], &HashMap::new()).is_empty());
    }

    #[test]
    fn swap_to_spare_kit_generates_issue_and_return() {
        let mut spare = mic(19, 1009, 2009, Some(3009));
        spare.is_spare = true;
        let k = kits(vec![mic(10, 1000, 2000, Some(3000)), spare]);
        let a = A::new().def(1, 10).ovr(1, Some(19), 102).0;
        let plan = handoff_plan(&songs(2), &a, &k);
        let t = plan.iter().find(|t| t.to_song == Some(102)).unwrap();
        assert!(t.handoffs.contains(&Handoff::Issue {
            item_id: 2009,
            to_performer: 1,
            to_kit: 19
        }));
        assert!(t.handoffs.contains(&Handoff::Return {
            item_id: 2000,
            from_performer: 1
        }));
    }

    #[test]
    fn handoff_performers_helper() {
        let h = Handoff::Transfer {
            item_id: 1,
            from_performer: 7,
            to_performer: 8,
            to_kit: 2,
        };
        assert_eq!(h.performers(), vec![7, 8]);
    }

    // ---------- upcoming ----------

    #[test]
    fn upcoming_from_current_song() {
        let k = kits(vec![mic(10, 1000, 2000, None)]);
        let a = A::new().def(1, 10).ovr(1, None, 102).0;
        let s = songs(4);
        let plan = handoff_plan(&s, &a, &k);
        // transitions: pre->101 issue, 101->102 return, 102->103 issue, 104->end return
        assert_eq!(plan.len(), 4);
        let up = upcoming_transitions(&plan, &s, Some(101), 2);
        assert_eq!(up.len(), 2);
        assert_eq!(up[0].from_song, Some(101));
        assert_eq!(up[1].from_song, Some(102));
        let pre = upcoming_transitions(&plan, &s, None, 1);
        assert_eq!(pre[0].from_song, None);
        let last = upcoming_transitions(&plan, &s, Some(104), 5);
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].to_song, None);
    }
}
