<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { timeAgo } from '../lib/format';
  import PerformerSheet from './PerformerSheet.svelte';
  import type { Alert, Performer } from '../lib/types';

  let { alerts, compact = false }: { alerts: Alert[]; compact?: boolean } = $props();

  let resolving = $state<Alert | null>(null);
  let swapFor = $state<{ performer: Performer; alert: Alert } | null>(null);

  /** The performer an alert concerns: named directly, or holding the alerted item now. */
  function performerFor(a: Alert): Performer | null {
    if (a.performer_id != null) return store.performers.get(a.performer_id) ?? null;
    if (a.equipment_id == null) return null;
    for (const [p, ks] of Object.entries(store.show?.effective ?? {})) {
      for (const k of ks) {
        const kit = store.kits.get(k);
        if (kit && [kit.carrier_id, kit.element_id, kit.base_id].includes(a.equipment_id)) return store.performers.get(Number(p)) ?? null;
      }
    }
    return null;
  }
  let note = $state('');
  let showBattery = $state(false);

  /** Automatic battery alerts are grouped into one row so they don't bury real problems. */
  const isBatteryRule = (a: Alert) => a.source === 'rule' && a.title.startsWith('Battery');
  const battery = $derived(alerts.filter(isBatteryRule));
  const batteryCritical = $derived(battery.filter((a) => a.severity === 'critical'));
  const listed = $derived(alerts.filter((a) => !isBatteryRule(a) || a.severity === 'critical' || showBattery));

  function ack(a: Alert) {
    return store.act(() => post(`/api/alerts/${a.id}/ack`));
  }
  async function resolve() {
    if (!resolving) return;
    const id = resolving.id;
    const n = note.trim() || null;
    resolving = null;
    note = '';
    await store.act(() => post(`/api/alerts/${id}/resolve`, { note: n }));
  }
</script>

<ul class="alerts" class:compact data-testid="alert-list">
  {#if battery.length - batteryCritical.length > 0}
    <li class="group">
      <button class="grouprow" onclick={() => (showBattery = !showBattery)} data-testid="battery-group">
        🔋 <strong>{battery.length} pack{battery.length === 1 ? '' : 's'} need a battery check</strong>
        <span class="muted">{showBattery ? 'Hide' : 'Show'}</span>
      </button>
    </li>
  {/if}
  {#each listed as a (a.id)}
    <li class="alert {a.severity}" class:acked={a.state === 'acknowledged'} data-testid="alert" data-state={a.state}>
      <div class="body">
        <div class="row">
          <span class="pill {a.severity === 'critical' ? 'crit' : a.severity === 'warning' ? 'warn' : 'info'}">{a.severity}</span>
          <strong class="title">{a.title}</strong>
        </div>
        <div class="muted meta">
          {#if a.performer_id}{store.performerName(a.performer_id)} · {/if}
          {#if a.equipment_id}{store.label(a.equipment_id)} · {/if}
          {a.opened_by}, {timeAgo(a.opened_at, store.now)}
          {#if a.state === 'acknowledged'} · <span data-testid="acked-by">ack {a.acked_by}</span>{/if}
        </div>
        {#if a.detail && !compact && a.source !== 'rule'}<div class="detail">{a.detail}</div>{/if}
      </div>
      <div class="actions">
        {#if !compact && a.source !== 'rule' && performerFor(a)}
          {@const p = performerFor(a)!}
          <button class="big" onclick={() => (swapFor = { performer: p, alert: a })} disabled={!store.connected || !store.canEdit} data-testid="alert-swap">⇄ Swap</button>
        {/if}
        {#if a.state === 'open'}
          <button class="big warn" onclick={() => ack(a)} disabled={!store.connected || !store.canEdit}>Ack</button>
        {/if}
        <button class="big" onclick={() => (resolving = a)} disabled={!store.connected || !store.canEdit}>Resolve</button>
      </div>
    </li>
  {:else}
    {#if battery.length === 0}<li class="muted empty">No active alerts</li>{/if}
  {/each}
</ul>

{#if swapFor}
  <PerformerSheet performer={swapFor.performer} alertId={swapFor.alert.id} onclose={() => (swapFor = null)} />
{/if}

{#if resolving}
  <div class="resolve panel">
    <strong>Resolve: {resolving.title}</strong>
    <input placeholder="What fixed it? (optional)" bind:value={note} aria-label="Resolution note" />
    <div class="row">
      <button class="primary big" onclick={resolve}>Resolve</button>
      <button class="big" onclick={() => (resolving = null)}>Cancel</button>
    </div>
  </div>
{/if}

<style>
  .alerts { list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }
  .grouprow { width: 100%; min-height: var(--touch); display: flex; gap: 8px; align-items: center; justify-content: space-between; text-align: left; font-weight: 400; border-color: color-mix(in srgb, var(--warn) 55%, var(--line)); }
  .alert { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; padding: 10px; border-radius: 10px; border: 1px solid var(--line); background: var(--panel-2); }
  .alert.critical { border-color: var(--crit); background: color-mix(in srgb, var(--crit) 18%, var(--panel-2)); }
  .alert.critical:not(.acked) { animation: pulse 1.2s ease-in-out infinite; }
  .alert.warning { border-color: color-mix(in srgb, var(--warn) 60%, transparent); }
  .body { flex: 1 1 180px; min-width: 0; }
  .title { overflow: hidden; text-overflow: ellipsis; }
  .meta { font-size: 0.82rem; }
  .detail { margin-top: 4px; font-size: 0.9rem; }
  .actions { display: flex; gap: 6px; margin-left: auto; }
  .empty { padding: 8px; }
  .resolve { margin-top: 8px; display: grid; gap: 8px; }
  @keyframes pulse { 50% { box-shadow: 0 0 0 4px color-mix(in srgb, var(--crit) 45%, transparent); } }
  @media (prefers-reduced-motion: reduce) { .alert.critical:not(.acked) { animation: none; } }
</style>
