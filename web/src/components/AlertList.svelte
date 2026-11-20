<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { timeAgo } from '../lib/format';
  import type { Alert } from '../lib/types';

  let { alerts, compact = false }: { alerts: Alert[]; compact?: boolean } = $props();

  let resolving = $state<Alert | null>(null);
  let note = $state('');

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
  {#each alerts as a (a.id)}
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
        {#if a.detail && !compact}<div class="detail">{a.detail}</div>{/if}
      </div>
      <div class="actions">
        {#if a.state === 'open'}
          <button class="big warn" onclick={() => ack(a)} disabled={!store.connected || !store.canEdit}>Ack</button>
        {/if}
        <button class="big" onclick={() => (resolving = a)} disabled={!store.connected || !store.canEdit}>Resolve</button>
      </div>
    </li>
  {:else}
    <li class="muted empty">No active alerts</li>
  {/each}
</ul>

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
  .alert { display: flex; gap: 10px; align-items: center; padding: 10px; border-radius: 10px; border: 1px solid var(--line); background: var(--panel-2); }
  .alert.critical { border-color: var(--crit); background: color-mix(in srgb, var(--crit) 18%, var(--panel-2)); }
  .alert.critical:not(.acked) { animation: pulse 1.2s ease-in-out infinite; }
  .alert.warning { border-color: color-mix(in srgb, var(--warn) 60%, transparent); }
  .body { flex: 1; min-width: 0; }
  .title { overflow: hidden; text-overflow: ellipsis; }
  .meta { font-size: 0.82rem; }
  .detail { margin-top: 4px; font-size: 0.9rem; }
  .actions { display: flex; gap: 6px; }
  .empty { padding: 8px; }
  .resolve { margin-top: 8px; display: grid; gap: 8px; }
  @keyframes pulse { 50% { box-shadow: 0 0 0 4px color-mix(in srgb, var(--crit) 45%, transparent); } }
  @media (prefers-reduced-motion: reduce) { .alert.critical:not(.acked) { animation: none; } }
</style>
