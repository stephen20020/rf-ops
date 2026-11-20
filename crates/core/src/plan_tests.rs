//! Scenario tests for the core rules: every case traced by the third and fourth reviews (a–m,
//! B-1…B-8) and owner decisions R1–R8.

use super::*;

// People
const HELEN: Id = 1;
const SHOSH: Id = 2;
const JUDAH: Id = 3;
const X: Id = 4;
const CHANDLER: Id = 5;

// Devices
const RED: Id = 10;
const TAN: Id = 11;
const BPACK3: Id = 12;
const RX10: Id = 20;
const RX9: Id = 21;
const RX11: Id = 22;
const RX1: Id = 23;
const RX3: Id = 24;
const RX6: Id = 25;
const GUEST: Id = 30;

// Bases (receiver channels / IEM TXs)
const TX1: Id = 201;
const TX9: Id = 209;
const TX10: Id = 210;
const TX_SPARE: Id = 299;

const PASS_IWFJ: Id = 900;

fn dev(id: Id, label: &str, kind: Kind, base: Id, spare: bool) -> Device {
    Device {
        id,
        label: label.into(),
        kind,
        base: Some(base),
        element: None,
        is_spare: spare,
        guest_owner: None,
    }
}

/// Six songs; Helen on Red and RX10 by default; RX10 passes to Shosh during song 3 ("IWFJ").
fn show() -> ShowState {
    let mut s = ShowState {
        devices: vec![
            dev(RED, "Red", Kind::Mic, 101, false),
            dev(TAN, "Tan", Kind::Mic, 102, false),
            dev(BPACK3, "Bpack3", Kind::Mic, 103, true),
            dev(RX10, "RX10", Kind::Iem, TX10, false),
            dev(RX9, "RX9", Kind::Iem, TX9, false),
            dev(RX11, "RX11", Kind::Iem, TX9, true),
            dev(RX1, "RX1", Kind::Iem, TX1, false),
            dev(RX3, "RX3", Kind::Iem, TX1, false),
            dev(RX6, "RX6", Kind::Iem, TX1, false),
        ],
        songs: (1..=6).map(|id| Song { id, skipped: false }).collect(),
        ..Default::default()
    };
    for d in s.devices.clone() {
        s.carriers.insert(d.id, 1000 + d.id);
    }
    s.cells.insert((RED, None), Some(HELEN));
    s.cells.insert((RX10, None), Some(HELEN));
    s.passes.push(Pass {
        id: PASS_IWFJ,
        device: RX10,
        song: 3,
        seq: 1,
        to: Some(SHOSH),
    });
    s.base_mix.insert(TX1, "ME 1".into());
    s.base_mix.insert(TX9, "Console mix".into());
    s.base_mix.insert(TX10, "ME 3".into());
    s
}

fn visit(s: &mut ShowState, song: Id) -> u32 {
    let no = s.visits.len() as u32 + 1;
    s.visits.push(Visit { no, song });
    no
}

fn hold(s: &mut ShowState, d: Id, p: Id) {
    s.holdings.insert(
        d,
        Holding {
            holder: Holder::Person(p),
            ..Default::default()
        },
    );
}

fn apply(s: &mut ShowState, moves: Moves) {
    for (d, h) in moves {
        s.holdings.insert(d, h);
    }
}

fn swap(id: Id, performer: Option<Id>, failed: Id, repl: Id, range: SubRange, v: u32) -> Substitution {
    Substitution {
        id,
        kind: SubKind::Swap,
        performer,
        failed: Some(failed),
        replacement: Some(repl),
        failed_base: None,
        replacement_base: None,
        retune: false,
        range,
        state: SubState::InHand,
        from_visit: v,
        starts_at_song: None,
        ended_at_visit: None,
    }
}

fn now_for(b: &Board, d: Id) -> Vec<Action> {
    b.now.iter().filter(|h| h.device == d).map(|h| h.action.clone()).collect()
}
fn next_for(b: &Board, d: Id) -> Vec<Action> {
    b.coming_up.iter().filter(|h| h.device == d).map(|h| h.action.clone()).collect()
}

// ---------------------------------------------------------------- basics

#[test]
fn pre_show_nothing_overdue_first_song_coming_up() {
    let s = show();
    let b = s.board();
    assert!(b.now.is_empty());
    assert_eq!(next_for(&b, RED), vec![Action::Give { to: HELEN }]);
    assert_eq!(next_for(&b, RX10), vec![Action::Give { to: HELEN }]);
}

#[test]
fn handoff_done_is_one_tap_r8() {
    let mut s = show();
    let b = s.board();
    let h = b.coming_up.iter().find(|h| h.device == RED).unwrap().clone();
    let m = s.do_handoff(&h);
    apply(&mut s, m);
    assert_eq!(s.holding(RED).holder, Holder::Person(HELEN));
    assert!(next_for(&s.board(), RED).is_empty());
}

// ---------------------------------------------------------------- (a) pass and return

#[test]
fn a_pass_during_song_then_return_at_next_song() {
    let mut s = show();
    hold(&mut s, RX10, HELEN);
    visit(&mut s, 1);
    visit(&mut s, 2);
    // One song ahead: prep card.
    let b = s.board();
    assert!(b.passes.iter().any(|p| p.pass == PASS_IWFJ && p.prep));
    visit(&mut s, 3);
    let b = s.board();
    assert!(now_for(&b, RX10).is_empty(), "Helen has it at the start of the song");
    let card = b.passes.iter().find(|p| !p.prep && p.pass == PASS_IWFJ).unwrap().clone();
    assert_eq!((card.from, card.to), (Some(HELEN), Some(SHOSH)));
    let (moves, mark) = s.pass_done(&card);
    apply(&mut s, moves);
    s.pass_marks.insert(mark);
    let b = s.board();
    assert!(now_for(&b, RX10).is_empty(), "Shosh has it after the pass: nothing overdue");
    assert_eq!(
        next_for(&b, RX10),
        vec![Action::Pass { from: SHOSH, to: HELEN }],
        "return is an ordinary handoff"
    );
}

// ---------------------------------------------------------------- (b) swap before a pass, B-1

#[test]
fn b_swap_before_pass_moves_the_pass_to_the_spare() {
    let mut s = show();
    hold(&mut s, RX10, HELEN);
    let v = visit(&mut s, 2);
    s.subs.push(swap(1, Some(HELEN), RX10, RX9, SubRange::UntilFixed, v));
    let m = s.sub_in_hand(&s.subs[0].clone(), HELEN);
    apply(&mut s, m);
    assert_eq!(s.holding(RX9).holder, Holder::Person(HELEN));
    assert_eq!(s.holding(RX10).holder, Holder::Stock);
    visit(&mut s, 3);
    let b = s.board();
    let card = b.passes.iter().find(|p| !p.prep).unwrap();
    assert_eq!(card.device, RX9, "the pass happens on the spare");
    assert!(now_for(&b, RX10).is_empty(), "the failed pack is never handed out");
    assert!(now_for(&b, RX9).is_empty());
    assert_eq!(s.planned(RX10, 3, 3, Some(1)), None);
    assert_eq!(s.planned(RX9, 3, 3, Some(1)), Some(SHOSH));
}

// ---------------------------------------------------------------- (c) swap after a pass, B-1

#[test]
fn c_swap_after_pass_keeps_the_phase_on_the_replacement() {
    let mut s = show();
    hold(&mut s, RX10, SHOSH);
    let v = visit(&mut s, 3);
    s.pass_marks.insert((PASS_IWFJ, v));
    s.subs.push(swap(1, Some(SHOSH), RX10, RX11, SubRange::ThisSong, v));
    let m = s.sub_in_hand(&s.subs[0].clone(), SHOSH);
    apply(&mut s, m);
    let b = s.board();
    assert!(now_for(&b, RX11).is_empty(), "no overdue 'pass RX11 Shosh → Helen' (B-1)");
    assert_eq!(s.planned(RX11, 3, v, None), Some(SHOSH));
    assert_eq!(s.holding(RX10).holder, Holder::Stock, "Shosh handed the failed pack back");
    // This-song swap ends with the song: next song Helen gets... the plan says RX10 (failed,
    // but not marked unusable in this test) — and RX11 is collected from Shosh.
    assert_eq!(next_for(&b, RX11), vec![Action::Collect { from: SHOSH }]);
}

// ---------------------------------------------------------------- (d) rest of show, B-3, B-5

#[test]
fn d_rest_of_show_never_overwrites_and_reassign_resolves_it() {
    let mut s = show();
    s.cells.insert((TAN, Some(5)), Some(X));
    s.equipment.insert(1000 + RED, Usability::Out);
    let v = visit(&mut s, 2);
    s.subs.push(swap(1, None, RED, TAN, SubRange::RestOfShow, v));
    let (c5, conf) = s.chains(5, v);
    assert_eq!(c5[&TAN].holders, vec![Some(X)], "X keeps Tan at song 5");
    assert!(conf.iter().any(|c| matches!(c, Conflict::ReplacementTaken { song: 5, device: TAN, .. })));
    // Elsewhere the replacement works.
    assert_eq!(s.planned(TAN, 4, v, None), Some(HELEN));
    // A2 resolves it: X uses Bpack3 instead (reassign, no plot edit).
    let mut r = swap(2, Some(X), TAN, BPACK3, SubRange::RestOfShow, v);
    r.kind = SubKind::Reassign;
    s.subs.push(r);
    let (c5, conf) = s.chains(5, v);
    assert!(!conf.iter().any(|c| matches!(c, Conflict::ReplacementTaken { .. })));
    assert_eq!(c5[&TAN].holders, vec![Some(HELEN)]);
    assert_eq!(c5[&BPACK3].holders, vec![Some(X)]);
}

#[test]
fn two_overlays_on_one_device_conflict_instead_of_overwriting() {
    let mut s = show();
    s.cells.insert((TAN, None), Some(JUDAH));
    s.cells.insert((TAN, Some(1)), None);
    let v = visit(&mut s, 2);
    s.subs.push(swap(1, Some(HELEN), RED, BPACK3, SubRange::UntilFixed, v));
    s.subs.push(swap(2, Some(JUDAH), TAN, BPACK3, SubRange::UntilFixed, v));
    let (c, conf) = s.chains(3, v);
    assert_eq!(c[&BPACK3].holders, vec![Some(HELEN)]);
    assert!(conf.iter().any(|c| matches!(c, Conflict::ReplacementTaken { sub: 2, .. })));
}

// ---------------------------------------------------------------- (e) unusable later, R2, B-6

#[test]
fn e_suspect_device_planned_later_warns_a_song_ahead_and_is_never_given() {
    let mut s = show();
    s.cells.insert((RED, Some(6)), Some(JUDAH));
    s.equipment.insert(1000 + RED, Usability::Suspect);
    hold(&mut s, RED, HELEN);
    let v = visit(&mut s, 2);
    s.subs.push(swap(1, Some(HELEN), RED, BPACK3, SubRange::UntilFixed, v));
    let m = s.sub_in_hand(&s.subs[0].clone(), HELEN);
    apply(&mut s, m);
    visit(&mut s, 3);
    visit(&mut s, 4);
    let b = s.board();
    assert!(!b.conflicts.iter().any(|c| matches!(c, Conflict::UnusablePlanned { .. })), "not yet");
    visit(&mut s, 5);
    let b = s.board();
    assert!(b.conflicts.iter().any(|c| matches!(
        c,
        Conflict::UnusablePlanned { song: 6, device: RED, performer: JUDAH }
    )));
    assert!(next_for(&b, RED).is_empty(), "no 'give Red to Judah'");
    // [Give Judah the same spare] while Helen still uses Bpack3 → offer another: Tan, planned.
    let mut ext = swap(2, Some(JUDAH), RED, TAN, SubRange::UntilFixed, 5);
    ext.state = SubState::Planned;
    ext.starts_at_song = Some(6);
    s.subs.push(ext);
    let b = s.board();
    assert_eq!(next_for(&b, TAN), vec![Action::Give { to: JUDAH }]);
    assert!(!b.conflicts.iter().any(|c| matches!(c, Conflict::UnusablePlanned { .. })));
    // B-4: a planned (future) swap doesn't hide devices or move anything now.
    assert!(b.held_by_swap.is_empty());
}

#[test]
fn unusable_base_or_element_counts_too() {
    let mut s = show();
    s.cells.insert((TAN, Some(4)), Some(JUDAH));
    s.equipment.insert(102, Usability::Suspect); // Tan's receiver channel
    visit(&mut s, 3);
    let b = s.board();
    assert!(next_for(&b, TAN).is_empty());
    assert!(b.conflicts.iter().any(|c| matches!(c, Conflict::UnusablePlanned { device: TAN, .. })));
}

// ---------------------------------------------------------------- (f) pull, B-5

#[test]
fn f_pull_mid_song_collects_at_the_boundary_not_on_stage() {
    let mut s = show();
    hold(&mut s, RED, HELEN);
    let v = visit(&mut s, 2);
    let mut p = swap(1, Some(HELEN), RED, RED, SubRange::UntilFixed, v);
    p.kind = SubKind::Pull;
    p.replacement = None;
    s.subs.push(p);
    let b = s.board();
    assert!(now_for(&b, RED).is_empty(), "no overdue collect while she's on stage");
    assert_eq!(next_for(&b, RED), vec![Action::Collect { from: HELEN }]);
    assert_eq!(s.usage(RED), Usage::Pulled);
}

// ---------------------------------------------------------------- (g) jump and back, R3, B-2

#[test]
fn g_jump_over_a_pass_then_go_back_redoes_the_song() {
    let mut s = show();
    hold(&mut s, RX10, HELEN);
    visit(&mut s, 2);
    visit(&mut s, 4); // jump 2 → 4: song 3 never visited
    let b = s.board();
    assert!(b.passes.iter().all(|p| p.prep), "no actionable song-3 pass");
    assert!(now_for(&b, RX10).is_empty());
    let v = visit(&mut s, 3); // back to 3 (R3: redo it)
    let b = s.board();
    assert!(b.passes.iter().any(|p| !p.prep && p.pass == PASS_IWFJ), "pass planned again");
    // Do it, then go back to 3 once more: Helen needs it at the start → overdue pass back.
    let card = b.passes.iter().find(|p| !p.prep).unwrap().clone();
    let (m, mark) = s.pass_done(&card);
    apply(&mut s, m);
    s.pass_marks.insert(mark);
    assert_eq!(mark.1, v);
    visit(&mut s, 3);
    let b = s.board();
    assert_eq!(now_for(&b, RX10), vec![Action::Pass { from: SHOSH, to: HELEN }]);
}

#[test]
fn this_song_swap_does_not_come_back_on_a_later_visit() {
    let mut s = show();
    let v = visit(&mut s, 2);
    s.subs.push(swap(1, Some(HELEN), RED, BPACK3, SubRange::ThisSong, v));
    assert_eq!(s.planned(BPACK3, 2, v, None), Some(HELEN));
    visit(&mut s, 3);
    let v2 = visit(&mut s, 2);
    assert_eq!(s.planned(BPACK3, 2, v2, None), None);
    assert_eq!(s.planned(RED, 2, v2, None), Some(HELEN));
}

#[test]
fn returned_swap_stays_ended_when_going_back() {
    let mut s = show();
    let v = visit(&mut s, 2);
    s.subs.push(swap(1, Some(HELEN), RED, BPACK3, SubRange::UntilFixed, v));
    visit(&mut s, 3);
    let r = visit(&mut s, 5);
    s.subs[0].state = SubState::Returned;
    s.subs[0].ended_at_visit = Some(r);
    let back = visit(&mut s, 3);
    assert_eq!(s.planned(RED, 3, back, None), Some(HELEN), "B-2: no re-applying after return");
    assert_eq!(s.planned(BPACK3, 3, back, None), None);
    // But an until-fixed swap still covers going back before it was returned.
    let mut s2 = show();
    let v = visit(&mut s2, 4);
    s2.subs.push(swap(1, Some(HELEN), RED, BPACK3, SubRange::UntilFixed, v));
    let back = visit(&mut s2, 2);
    assert_eq!(s2.planned(BPACK3, 2, back, None), Some(HELEN), "R1: until fixed");
}

// ---------------------------------------------------------------- (h) TX swap, B-7

#[test]
fn h_tx_swap_retunes_every_pack_and_keeps_the_mix() {
    let mut s = show();
    for (d, p) in [(RX1, 41), (RX3, 43), (RX6, 46)] {
        s.cells.insert((d, None), Some(p));
        hold(&mut s, d, p);
    }
    let v = visit(&mut s, 2);
    assert!(now_for(&s.board(), RX1).is_empty());
    s.subs.push(Substitution {
        id: 7,
        kind: SubKind::TxSwap,
        performer: None,
        failed: None,
        replacement: None,
        failed_base: Some(TX1),
        replacement_base: Some(TX_SPARE),
        retune: false,
        range: SubRange::UntilFixed,
        state: SubState::InHand,
        from_visit: v,
        starts_at_song: None,
        ended_at_visit: None,
    });
    let b = s.board();
    for d in [RX1, RX3, RX6] {
        assert_eq!(now_for(&b, d), vec![Action::Retune { to_base: Some(TX_SPARE) }], "pack {d}");
        assert_eq!(s.mix(d, 2, v).as_deref(), Some("ME 1"), "TX9 carries TX1's mix");
    }
    // Per-pack checklist: retune one, it disappears.
    let h = b.now.iter().find(|h| h.device == RX3).unwrap().clone();
    let m = s.do_handoff(&h);
    apply(&mut s, m);
    assert!(now_for(&s.board(), RX3).is_empty());
}

#[test]
fn tx_swap_onto_a_tx_feeding_another_pack_conflicts() {
    let mut s = show();
    s.cells.insert((RX1, None), Some(41));
    s.cells.insert((RX9, None), Some(49));
    let v = visit(&mut s, 2);
    s.subs.push(Substitution {
        id: 7,
        kind: SubKind::TxSwap,
        performer: None,
        failed: None,
        replacement: None,
        failed_base: Some(TX1),
        replacement_base: Some(TX9),
        retune: false,
        range: SubRange::UntilFixed,
        state: SubState::InHand,
        from_visit: v,
        starts_at_song: None,
        ended_at_visit: None,
    });
    let (_, conf) = s.chains(2, v);
    assert!(conf.iter().any(|c| matches!(c, Conflict::TxTaken { base: TX9, .. })));
}

// ---------------------------------------------------------------- (i) IEM retune, R6

#[test]
fn i_spare_pack_is_retuned_to_the_failed_packs_tx_and_back_after() {
    let mut s = show();
    hold(&mut s, RX10, HELEN);
    let v = visit(&mut s, 1);
    let mut sw = swap(1, Some(HELEN), RX10, RX11, SubRange::UntilFixed, v);
    sw.retune = true;
    sw.state = SubState::Recorded;
    s.subs.push(sw);
    // B-4: while recorded, both devices are left to the swap card.
    let b = s.board();
    assert!(b.held_by_swap.contains(&RX10) && b.held_by_swap.contains(&RX11));
    assert!(now_for(&b, RX11).is_empty() && now_for(&b, RX10).is_empty());
    assert_eq!(s.effective_base(RX11, 1, v), Some(TX10));
    assert_eq!(s.mix(RX11, 1, v).as_deref(), Some("ME 3"), "Helen keeps her mix");
    let m = s.sub_in_hand(&s.subs[0].clone(), HELEN);
    apply(&mut s, m);
    s.subs[0].state = SubState::InHand;
    assert!(now_for(&s.board(), RX11).is_empty(), "synced to TX10 at In hand");
    // Return at song 2: RX10 back to Helen, RX11 to stock, then "retune RX11 back to TX9".
    let r = visit(&mut s, 2);
    let m = s.return_in_hand(&s.subs[0].clone(), HELEN);
    apply(&mut s, m);
    s.subs[0].state = SubState::Returned;
    s.subs[0].ended_at_visit = Some(r);
    let b = s.board();
    assert_eq!(s.holding(RX10).holder, Holder::Person(HELEN));
    assert_eq!(now_for(&b, RX11), vec![Action::Retune { to_base: Some(TX9) }]);
}

// ---------------------------------------------------------------- (j) carry-over, R4

#[test]
fn j_same_person_keeps_the_device_for_the_evening_show() {
    let mut s = show();
    hold(&mut s, RX10, HELEN);
    hold(&mut s, RED, HELEN);
    s.next_show_first = Some(BTreeMap::from([(RX10, Some(HELEN)), (RED, Some(JUDAH))]));
    visit(&mut s, 6);
    let b = s.board();
    assert_eq!(next_for(&b, RX10), vec![Action::Keep { with: HELEN }]);
    assert_eq!(next_for(&b, RED), vec![Action::Collect { from: HELEN }]);
    let out: Vec<Id> = s.count_in().into_iter().map(|(d, _)| d).collect();
    assert_eq!(out, vec![RED], "kept devices aren't counted as out");
}

// ---------------------------------------------------------------- (k) guest

#[test]
fn k_guest_replacement_moves_only_that_performers_cells() {
    let mut s = show();
    s.cells.insert((RX9, None), Some(CHANDLER));
    s.cells.insert((RX9, Some(5)), Some(JUDAH));
    s.devices.push(Device {
        guest_owner: Some(CHANDLER),
        ..dev(GUEST, "Chandler's pack", Kind::Iem, TX9, false)
    });
    s.carriers.insert(GUEST, 1030);
    let mut g = swap(3, Some(CHANDLER), RX9, GUEST, SubRange::RestOfShow, 0);
    g.kind = SubKind::Guest;
    s.subs.push(g);
    let v = visit(&mut s, 1);
    assert_eq!(s.planned(GUEST, 2, v, None), Some(CHANDLER));
    assert_eq!(s.planned(RX9, 2, v, None), None);
    assert_eq!(s.planned(RX9, 5, v, None), Some(JUDAH), "Judah keeps RX9 at song 5");
    // A guest's own device goes home with them at the end, never collected.
    hold(&mut s, GUEST, CHANDLER);
    visit(&mut s, 6);
    assert_eq!(next_for(&s.board(), GUEST), vec![Action::Keep { with: CHANDLER }]);
}

// ---------------------------------------------------------------- (l) several subs, one device

#[test]
fn l_swap_return_then_second_failure() {
    let mut s = show();
    let v1 = visit(&mut s, 2);
    s.subs.push(swap(1, Some(HELEN), RED, BPACK3, SubRange::UntilFixed, v1));
    let r = visit(&mut s, 3);
    s.subs[0].state = SubState::Returned;
    s.subs[0].ended_at_visit = Some(r);
    let v2 = visit(&mut s, 4);
    s.subs.push(swap(2, Some(HELEN), RED, TAN, SubRange::UntilFixed, v2));
    assert_eq!(s.planned(TAN, 5, v2, None), Some(HELEN));
    assert_eq!(s.planned(BPACK3, 5, v2, None), None);
    assert_eq!(s.planned(RED, 5, v2, None), None);
}

// ---------------------------------------------------------------- (m) in hand vs plot handoff

#[test]
fn m_in_hand_owns_the_move_and_no_duplicate_give_appears() {
    let mut s = show();
    hold(&mut s, RED, HELEN);
    let v = visit(&mut s, 2);
    let mut sw = swap(1, Some(HELEN), RED, BPACK3, SubRange::UntilFixed, v);
    sw.state = SubState::Recorded;
    s.subs.push(sw);
    let b = s.board();
    assert!(now_for(&b, BPACK3).is_empty(), "no separate 'give Bpack3' while recorded");
    let m = s.sub_in_hand(&s.subs[0].clone(), HELEN);
    apply(&mut s, m);
    s.subs[0].state = SubState::InHand;
    let b = s.board();
    assert!(now_for(&b, BPACK3).is_empty() && now_for(&b, RED).is_empty());
    assert_eq!(s.usage(BPACK3), Usage::OnSpareFor { failed: RED });
}

#[test]
fn in_hand_takes_the_replacement_from_whoever_holds_it() {
    let mut s = show();
    hold(&mut s, RED, HELEN);
    hold(&mut s, TAN, JUDAH);
    let v = visit(&mut s, 2);
    let sw = swap(1, Some(HELEN), RED, TAN, SubRange::ThisSong, v);
    let m = s.sub_in_hand(&sw, HELEN);
    assert!(m.contains(&(TAN, Holding { holder: Holder::Person(HELEN), ..Default::default() })));
}

// ---------------------------------------------------------------- conflicts, usage, batteries

#[test]
fn conflicts_two_packs_absent_and_element_twice() {
    let mut s = show();
    s.cells.insert((RX9, Some(2)), Some(HELEN));
    s.absent.insert(SHOSH);
    s.cell_elements.insert((RED, Some(2)), 500);
    s.cell_elements.insert((TAN, Some(2)), 500);
    s.cells.insert((TAN, Some(2)), Some(JUDAH));
    let (c, _) = s.chains(2, 1);
    let conf = s.song_conflicts(2, 1, &c);
    assert!(conf.iter().any(|c| matches!(c, Conflict::TwoPacks { performer: HELEN, .. })));
    assert!(conf.iter().any(|c| matches!(c, Conflict::ElementTwice { element: 500, .. })));
    let (c3, _) = s.chains(3, 1);
    assert!(s
        .song_conflicts(3, 1, &c3)
        .iter()
        .any(|c| matches!(c, Conflict::AbsentPlanned { performer: SHOSH, .. })));
}

#[test]
fn capsule_change_is_its_own_handoff() {
    let mut s = show();
    hold(&mut s, RED, HELEN);
    s.cell_elements.insert((RED, Some(3)), 777);
    visit(&mut s, 2);
    assert_eq!(
        next_for(&s.board(), RED),
        vec![Action::Capsule { holder: HELEN, element: Some(777) }]
    );
}

#[test]
fn usage_states() {
    let mut s = show();
    s.cells.insert((TAN, Some(5)), Some(JUDAH));
    visit(&mut s, 2);
    assert_eq!(s.usage(RED), Usage::InUse { by: HELEN });
    assert_eq!(s.usage(TAN), Usage::Next { song: 5, by: JUDAH });
    assert_eq!(s.usage(BPACK3), Usage::Spare);
    assert_eq!(s.usage(RX9), Usage::Done);
    hold(&mut s, RX9, SHOSH);
    assert_eq!(s.usage(RX9), Usage::Collect { from: SHOSH });
    s.equipment.insert(1000 + RX9, Usability::Missing);
    assert_eq!(s.usage(RX9), Usage::Missing);
}

#[test]
fn skipped_songs_are_passed_over() {
    let mut s = show();
    s.songs[2].skipped = true; // song 3
    assert_eq!(s.next_song_after(Some(2)), Some(4));
}

#[test]
fn battery_evidence_is_per_show_r5() {
    use chrono::TimeZone;
    let t = |h: u32| chrono::Utc.with_ymd_and_hms(2026, 6, 20, h, 0, 0).unwrap();
    // Evening call 18:00 → window from 16:00, but the afternoon show ended 17:00 → from 17:00.
    assert_eq!(battery_evidence_start(Some(t(18)), None, Some(t(17))), Some(t(17)));
    assert_eq!(battery_evidence_start(Some(t(18)), None, Some(t(12))), Some(t(16)));
    assert_eq!(battery_evidence_start(None, Some(t(15)), None), Some(t(15)));
}
