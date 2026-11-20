<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { consoleTag, KIND_LABEL } from '../lib/format';
  import BatteryBadge from './BatteryBadge.svelte';
  import type { Kit, Performer } from '../lib/types';

  let { performer, now, next, onopen }: { performer: Performer; now: Kit[]; next: Kit[] | null; onopen: () => void } = $props();

  const alerts = $derived(store.alertsFor(performer.id, [...now, ...(next ?? [])]));
  const critical = $derived(alerts.some((a) => a.severity === 'critical'));
  const offNow = $derived(now.length === 0);
  const changes = $derived(
    next !== null && (next.length !== now.length || next.some((k, i) => k.id !== now[i]?.id)),
  );
</script>

<button class="card" class:crit={critical} class:warned={!critical && alerts.length > 0} class:off={offNow} onclick={onopen} data-testid="performer-card" data-name={performer.name}>
  <div class="head">
    <div class="avatar" aria-hidden="true">{performer.name.slice(0, 1)}</div>
    <div class="who">
      <div class="name">{performer.name}</div>
      <div class="muted role">{performer.role}{performer.zone ? ` · ${performer.zone}` : ''}</div>
    </div>
    {#if alerts.length}<span class="pill {critical ? 'crit' : 'warn'}">⚠ {alerts.length}</span>{/if}
  </div>
  {#if offNow}
    <div class="muted offline">Not on in this song{#if next?.length} · <strong>on next</strong>{/if}</div>
  {/if}
  <ul class="kits">
    {#each (offNow ? (next ?? []) : now) as k (k.id)}
      <li class:dim={offNow}>
        <span class="pill">{KIND_LABEL[k.kind]}</span>
        <span class="tag">{consoleTag(k)}</span>
        <span class="eq" title={k.name}>{store.label(k.carrier_id)}{#if k.element_id}{' + '}{store.label(k.element_id)}{/if}</span>
        <BatteryBadge id={k.carrier_id} />
      </li>
    {/each}
  </ul>
  {#if changes && !offNow}
    <div class="change">Next song: {next?.length ? next.map((k) => k.name).join(', ') : 'off'}</div>
  {/if}
</button>

<style>
  .card { display: grid; grid-template-columns: minmax(0, 1fr); min-width: 0; overflow: hidden; gap: 8px; text-align: left; padding: 12px; background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius); font-weight: 400; width: 100%; }
  .card.crit { border-color: var(--crit); box-shadow: inset 0 0 0 2px var(--crit); }
  .card.warned { border-color: var(--warn); }
  .card.off { opacity: 0.7; }
  .head { display: flex; gap: 10px; align-items: center; }
  .avatar { width: 40px; height: 40px; border-radius: 50%; background: var(--panel-2); display: grid; place-items: center; font-weight: 750; font-size: 1.1rem; flex: none; }
  .who { flex: 1; min-width: 0; }
  .name { font-weight: 700; font-size: 1.1rem; }
  .role { font-size: 0.82rem; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .kits { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .kits li { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; grid-template-areas: 'kind tag batt' 'eq eq eq'; column-gap: 8px; row-gap: 2px; align-items: center; padding: 4px 0; border-top: 1px solid var(--line); }
  .kits li > :global(.pill:first-child) { grid-area: kind; }
  .kits li > :global(.pill:last-child) { grid-area: batt; }
  .kits li.dim { opacity: 0.75; }
  .tag { grid-area: tag; font-weight: 700; white-space: nowrap; }
  .eq { grid-area: eq; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 0.95rem; }
  .change { font-size: 0.85rem; color: var(--warn); font-weight: 600; }
  .offline { font-size: 0.85rem; }
</style>
