import type { BatteryDue, Handoff, HandoffDone, Id, Kit, KitKind } from './types';

export function handoffItem(h: Handoff): Id {
  return h.type === 'component_change' ? h.carrier_id : h.item_id;
}

export function doneKey(from: Id | null, to: Id | null, item: Id, type: string): string {
  return `${from ?? -1}:${to ?? -1}:${item}:${type}`;
}

export function doneMap(list: HandoffDone[]): Map<string, HandoffDone> {
  return new Map(list.map((d) => [doneKey(d.from_song, d.to_song, d.item_id, d.handoff_type), d]));
}

/** Performers involved in a handoff, for filtering by A2 ownership. */
export function handoffPerformers(h: Handoff): Id[] {
  switch (h.type) {
    case 'transfer':
      return [h.from_performer, h.to_performer];
    case 'issue':
      return [h.to_performer];
    case 'return':
      return [h.from_performer];
    case 'component_change':
      return [h.performer_id];
  }
}

export const KIND_LABEL: Record<KitKind, string> = { mic: 'Mic', iem: 'IEM', guitar: 'Guitar' };

export const DUE_LABEL: Record<BatteryDue, string> = {
  ok: 'OK',
  due_soon: 'Due soon',
  overdue: 'Overdue',
  low: 'LOW',
  unchecked: 'Unchecked',
};

export function kitSummary(k: Kit, label: (id: Id | null) => string): string {
  const parts = [label(k.carrier_id)];
  if (k.element_id != null) parts.push(label(k.element_id));
  return parts.join(' + ');
}

export function consoleTag(k: Kit): string {
  if (k.kind === 'iem') return k.iem_mix ? k.iem_mix : 'IEM';
  return k.console_channel != null ? `Ch ${k.console_channel}` : 'Ch —';
}

export function timeAgo(iso: string | null, now: number): string {
  if (!iso) return '';
  const s = Math.max(0, Math.floor((now - Date.parse(iso)) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m ago`;
}

export function clock(iso: string | null): string {
  if (!iso) return '';
  return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}
