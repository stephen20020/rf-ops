// The v3.1 board: core rules applied to the selected show (server /api/shows/{id}/board), plus
// swaps and battery checks. Reloads on every live event.
import { get, post } from './api';
import { store } from './store.svelte';
import type { Id } from './types';

export type Holder = { kind: 'stock' } | { kind: 'person'; id: Id } | { kind: 'owner' };
export type Action =
  | { type: 'give'; to: Id }
  | { type: 'collect'; from: Id }
  | { type: 'pass'; from: Id; to: Id }
  | { type: 'capsule'; holder: Id; element: Id | null }
  | { type: 'retune'; to_base: Id | null }
  | { type: 'keep'; with: Id };
export interface Handoff { device: Id; target: 'now' | 'coming_up'; song: Id | null; action: Action; via_sub: Id | null }
export interface PassCard { pass: Id; device: Id; from: Id | null; to: Id | null; prep: boolean }
export type Usage =
  | { usage: 'out' } | { usage: 'suspect' } | { usage: 'missing' } | { usage: 'pulled' }
  | { usage: 'on_spare_for'; failed: Id } | { usage: 'in_use'; by: Id } | { usage: 'collect'; from: Id }
  | { usage: 'next'; song: Id; by: Id } | { usage: 'spare' } | { usage: 'done' };
export interface Conflict { type: string; song?: Id; device?: Id; performer?: Id; devices?: Id[]; sub?: Id; planned_for?: Id | null; base?: Id }
export interface Tile { id: Id; label: string; kind: 'mic' | 'iem' | 'guitar'; is_spare: boolean; usage: Usage; holder: Holder; mix: string | null }
export interface BoardData {
  board: {
    current_song: Id | null; visit: number; now: Handoff[]; coming_up: Handoff[];
    passes: PassCard[]; conflicts: Conflict[]; held_by_swap: Id[];
  };
  devices: Tile[];
  count_in: [Id, Holder][];
  names: { devices: Record<string, string>; people: Record<string, string>; songs: Record<string, string>; equipment: Record<string, string> };
}
export interface Swap {
  id: Id; kind: string; range: string; state: string; retune: boolean; active: boolean;
  performer: string | null; failed: string | null; failed_id: Id | null; replacement: string | null; replacement_id: Id | null;
  failed_channel: string | null; replacement_channel: string | null; mix: string | null;
  failed_base: string | null; replacement_base: string | null; created_by: string; created_at: string; component: string;
}
export interface Battery { result: 'ok' | 'fresh' | 'low'; at: string; by: string }

class Plan {
  data = $state<BoardData | null>(null);
  swaps = $state<Swap[]>([]);
  batteries = $state<Record<string, Battery>>({});
  /** Last Done, undoable for 10 s (R8). */
  undo = $state<{ seq: number; text: string; until: number } | null>(null);
  private loading = false;
  private again = false;

  constructor() {
    store.onRefresh(() => void this.load());
  }

  person(id: Id | null | undefined): string {
    if (id == null) return 'nobody';
    return this.data?.names.people[String(id)] ?? store.performerName(id);
  }
  device(id: Id | null | undefined): string {
    if (id == null) return '—';
    return this.data?.names.devices[String(id)] ?? `#${id}`;
  }
  song(id: Id | null | undefined): string {
    if (id == null) return 'after the show';
    return this.data?.names.songs[String(id)] ?? `#${id}`;
  }
  base(id: Id | null | undefined): string {
    if (id == null) return 'its own TX';
    return this.data?.names.equipment[String(id)] ?? `#${id}`;
  }

  /** Plain-English line for a handoff (verb first, R: "Give Red to Helen"). */
  text(h: Handoff): string {
    const d = this.device(h.device);
    const a = h.action;
    switch (a.type) {
      case 'give': return `Give ${d} to ${this.person(a.to)}`;
      case 'collect': return `Collect ${d} from ${this.person(a.from)}`;
      case 'pass': return `Pass ${d}: ${this.person(a.from)} → ${this.person(a.to)}`;
      case 'capsule': return `Change the capsule on ${d} (${this.person(a.holder)})`;
      case 'retune': return `Sync ${d} to ${this.base(a.to_base)}`;
      case 'keep': return `Keep ${d} with ${this.person(a.with)} for the next show`;
    }
  }

  /** Hygiene hint for a device changing person. */
  hint(h: Handoff): string {
    const kind = this.data?.devices.find((t) => t.id === h.device)?.kind;
    if (h.action.type === 'pass' || h.action.type === 'give') return kind === 'iem' ? 'Own earpieces · wipe the pack' : kind === 'mic' ? 'Wipe the handheld' : '';
    return '';
  }

  conflictText(c: Conflict): string {
    const s = c.song != null ? ` (${this.song(c.song)})` : '';
    switch (c.type) {
      case 'unusable_planned': return `${this.device(c.device)} is broken but planned for ${this.person(c.performer)}${s}`;
      case 'replacement_taken': return `${this.device(c.device)} is needed by ${this.person(c.planned_for)}${s} and is also replacing a broken device`;
      case 'two_packs': return `${this.person(c.performer)} has two packs${s}: ${(c.devices ?? []).map((d) => this.device(d)).join(', ')}`;
      case 'two_mics': return `${this.person(c.performer)} has two mics${s}`;
      case 'absent_planned': return `${this.person(c.performer)} is absent but planned on ${this.device(c.device)}${s}`;
      case 'pass_on_pulled': return `A pass on pulled ${this.device(c.device)}${s} has nothing to hand over`;
      case 'tx_taken': return `${this.base(c.base)} already feeds another pack${s}`;
      case 'element_twice': return `One capsule is planned on two mics${s}`;
      default: return c.type;
    }
  }

  async load() {
    const id = store.showId;
    if (!id) return;
    if (this.loading) {
      this.again = true;
      return;
    }
    this.loading = true;
    try {
      do {
        this.again = false;
        const [b, s, bat] = await Promise.all([
          get<BoardData>(`/api/shows/${id}/board`),
          get<Swap[]>(`/api/shows/${id}/swaps`),
          get<{ devices: Record<string, Battery> }>(`/api/shows/${id}/batteries`),
        ]);
        this.data = b;
        this.swaps = s;
        this.batteries = bat.devices;
      } while (this.again);
    } catch {
      /* the connection banner shows problems */
    } finally {
      this.loading = false;
    }
  }

  async done(h: Handoff) {
    const r = await store.act(() => post<{ seq: number }>(`/api/shows/${store.showId}/moves/done`, { device: h.device, action: h.action }));
    if (r) this.offerUndo(r.seq, this.text(h));
  }
  async passDone(p: PassCard) {
    const r = await store.act(() => post<{ seq: number }>(`/api/shows/${store.showId}/passes/${p.pass}/done`));
    if (r) this.offerUndo(r.seq, `Pass ${this.device(p.device)}: ${this.person(p.from)} → ${this.person(p.to)}`);
  }
  leave(h: Handoff) {
    return store.act(() => post(`/api/shows/${store.showId}/moves/leave`, { device: h.device }), 'Left for this song');
  }
  private offerUndo(seq: number, text: string) {
    const until = Date.now() + 10_000;
    this.undo = { seq, text, until };
    setTimeout(() => {
      if (this.undo?.seq === seq) this.undo = null;
    }, 10_000);
  }
  async doUndo() {
    const u = this.undo;
    if (!u) return;
    this.undo = null;
    await store.act(() => post(`/api/shows/${store.showId}/undo`, { seq: u.seq }), `Undone: ${u.text}`);
  }

  battery(device: Id, result: 'ok' | 'fresh' | 'low') {
    return store.act(() => post(`/api/devices/${device}/battery`, { result }), `${this.device(device)}: battery ${result}`);
  }
}

export const plan = new Plan();
