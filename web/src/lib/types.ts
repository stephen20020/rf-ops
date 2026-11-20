// Mirrors the server's JSON. Keep in sync with crates/server/src/routes/*.rs.

export type Id = number;

export type EquipmentKind =
  | 'receiver' | 'receiver_channel' | 'handheld_tx' | 'bodypack_tx' | 'capsule' | 'headset' | 'lav'
  | 'iem_tx' | 'iem_tx_channel' | 'iem_pack' | 'guitar_receiver' | 'guitar_pack' | 'battery';
export type EquipmentStatus = 'available' | 'checked_out' | 'in_use' | 'faulty' | 'retired';
export type KitKind = 'mic' | 'iem' | 'guitar';
export type Role = 'admin' | 'a2' | 'foh' | 'viewer';
export type Severity = 'info' | 'warning' | 'critical';
export type BatteryDue = 'ok' | 'due_soon' | 'overdue' | 'low' | 'unchecked';

export interface Equipment {
  id: Id; kind: EquipmentKind; make: string; model: string; label: string;
  serial: string | null; parent_id: Id | null; channel_no: number | null; ip_address: string | null;
  status: EquipmentStatus; notes: string;
}
export interface User { id: Id; name: string; role: Role; has_pin: boolean }
export interface Performer {
  id: Id; name: string; role: string; photo_path: string | null; notes: string;
  owner_user_id: Id | null; zone: string | null;
}
export interface Show {
  id: Id; name: string; starts_at: string | null; call_time: string | null;
  current_song_id: Id | null; mode: 'setup' | 'rehearsal' | 'show'; midi_mode: 'follow' | 'lock';
}
export interface Song {
  id: Id; show_id: Id; position: number; title: string; midi_trigger: string | null; skipped: boolean;
}
export interface Kit {
  id: Id; show_id: Id; kind: KitKind; name: string; base_id: Id | null; carrier_id: Id | null;
  element_id: Id | null; console_channel: number | null; console_channel_name: string | null;
  dca: string | null; iem_mix: string | null; is_spare: boolean;
}
export interface Assignment {
  id: Id; show_id: Id; performer_id: Id; kind: KitKind; kit_id: Id | null; song_id: Id | null;
}
export type Handoff =
  | { type: 'transfer'; item_id: Id; from_performer: Id; to_performer: Id; to_kit: Id }
  | { type: 'issue'; item_id: Id; to_performer: Id; to_kit: Id }
  | { type: 'return'; item_id: Id; from_performer: Id }
  | { type: 'component_change'; carrier_id: Id; from_element: Id | null; to_element: Id | null; performer_id: Id };
export interface Transition { from_song: Id | null; to_song: Id | null; handoffs: Handoff[] }
export interface HandoffDone {
  from_song: Id; to_song: Id; item_id: Id; handoff_type: Handoff['type']; done_by: string; done_at: string;
}
export type Conflict =
  | { type: 'item_shared'; item_id: Id; performer_ids: Id[]; kit_ids: Id[] }
  | { type: 'unknown_kit'; performer_id: Id; kit_id: Id }
  | { type: 'kind_mismatch'; assignment_id: Id; kit_id: Id; expected: KitKind; actual: KitKind }
  | { type: 'unusable_item'; item_id: Id; kit_id: Id; performer_id: Id; status: EquipmentStatus };
export interface ConflictReport {
  static_conflicts: Conflict[];
  by_song: { song_id: Id | null; conflicts: Conflict[] }[];
}
export interface BatteryStatus {
  equipment_id: Id; label: string; kind: EquipmentKind; due: BatteryDue;
  last_result: string | null; last_checked_at: string | null; last_checked_by: string | null;
}
export interface Alert {
  id: Id; show_id: Id | null; severity: Severity; source: string; equipment_id: Id | null;
  performer_id: Id | null; title: string; detail: string; state: 'open' | 'acknowledged' | 'resolved';
  opened_at: string; opened_by: string; acked_at: string | null; acked_by: string | null;
  resolved_at: string | null; resolved_by: string | null; resolution_note: string | null;
}
export interface Checkout {
  id: Id; equipment_id: Id; performer_id: Id | null; show_id: Id | null;
  out_at: string; out_by: string; in_at: string | null; in_by: string | null;
}
export type Effective = Record<string, Id[]>;

export interface ShowSnapshot {
  show: Show; songs: Song[]; kits: Kit[]; assignments: Assignment[];
  effective: Effective; next_effective: Effective | null; next_song_id: Id | null;
  handoff_plan: Transition[]; upcoming: Transition[]; handoffs_done: HandoffDone[];
  conflicts: ConflictReport; battery: BatteryStatus[]; recent_swaps: ServerEvent[];
}
export interface Snapshot {
  seq: number; server_time: string; users: User[]; performers: Performer[]; equipment: Equipment[];
  shows: Show[]; alerts: Alert[]; open_checkouts: Checkout[]; show: ShowSnapshot | null; midi: MidiStatus;
}

export interface ServerEvent {
  seq: number; at: string; actor: string; client_id: string | null; type: string; payload: unknown;
}
export type WsMessage =
  | { type: 'hello'; seq: number; server_time: string }
  | { type: 'event'; event: ServerEvent }
  | { type: 'resync'; seq: number };

export interface KitRef {
  id: Id; name: string; console_channel: number | null; console_channel_name: string | null; iem_mix: string | null;
}
export interface SwapPayload {
  show_id: Id; swap_id: Id; performer_id: Id; performer_name: string; from_kit: KitRef; to_kit: KitRef;
  scope: 'song' | 'rest_of_show'; songs_changed: Id[]; faulty_item_id: Id | null; reason: string; alert_id: Id | null;
}
export interface BackupFile { name: string; bytes: number; created_at: string }
export interface Report {
  show: Show; generated_at: string; alerts: Alert[];
  swaps: { id: Id; at: string; by: string; performer: string; from_kit: string | null; to_kit: string; scope: string; song: string | null; reason: string }[];
  battery_checks: { checked_at: string; checked_by: string; label: string; result: string; battery_type: string }[];
  faulty_items: { id: Id; label: string; kind: string; model: string; notes: string }[];
  song_changes: { at: string; actor: string; song: string | null; source: string | null }[];
  handoffs_done: number;
}

export type MidiTrigger =
  | { type: 'program_change'; channel: number; value: number }
  | { type: 'control_change'; channel: number; controller: number; value: number }
  | { type: 'note_on'; channel: number; note: number };
export interface MidiStatus {
  bridge: { name: string; port: string | null; ports: string[]; online: boolean; last_seen_at: string } | null;
  recent: { at: string; source: string; message: string; outcome: string }[];
  channel: number; next_trigger: MidiTrigger | null; prev_trigger: MidiTrigger | null; target_show_id: Id | null;
}
