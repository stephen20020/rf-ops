<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { consoleTag, KIND_LABEL } from '../lib/format';
  import BatteryBadge from './BatteryBadge.svelte';
  import Modal from './Modal.svelte';
  import type { Id, Kit, Performer } from '../lib/types';

  let {
    performer,
    from,
    alertId = null,
    onclose,
  }: { performer: Performer; from: Kit; alertId?: Id | null; onclose: () => void } = $props();

  let scope = $state<'rest_of_show' | 'song'>('rest_of_show');
  let markFaulty = $state(true);
  let busy = $state(false);

  /** kit id -> performer holding it in the current song */
  const holders = $derived.by(() => {
    const m = new Map<Id, Id>();
    for (const [p, ks] of Object.entries(store.show?.effective ?? {})) for (const k of ks) m.set(k, Number(p));
    return m;
  });
  const options = $derived(
    (store.show?.kits ?? [])
      .filter((k) => k.kind === from.kind && k.id !== from.id)
      .map((k) => ({
        kit: k,
        holder: holders.get(k.id) ?? null,
        faulty: [k.carrier_id, k.element_id, k.base_id].some((i) => i != null && ['faulty', 'retired'].includes(store.equipment.get(i)?.status ?? '')),
      }))
      .filter((o) => o.kit.is_spare || o.holder == null)
      .sort((a, b) => Number(b.kit.is_spare) - Number(a.kit.is_spare) || Number(a.holder != null) - Number(b.holder != null)),
  );

  async function swapTo(to: Kit) {
    busy = true;
    const r = await store.act(
      () =>
        post(`/api/shows/${store.showId}/swap`, {
          performer_id: performer.id,
          from_kit_id: from.id,
          to_kit_id: to.id,
          scope,
          mark_faulty: markFaulty,
          alert_id: alertId,
          reason: 'swap to spare',
        }),
      `${performer.name} → ${to.name}`,
    );
    busy = false;
    if (r) onclose();
  }
</script>

<Modal title="Swap {performer.name} off {from.name}" {onclose}>
  <div class="row opts">
    <button class:primary={scope === 'rest_of_show'} onclick={() => (scope = 'rest_of_show')}>Rest of show</button>
    <button class:primary={scope === 'song'} onclick={() => (scope = 'song')}>This song only</button>
    <label class="chk"><input type="checkbox" bind:checked={markFaulty} /> Mark {store.label(from.carrier_id)} faulty</label>
  </div>
  <p class="muted small">Tap a kit to swap now. FOH sees the new channel immediately.</p>
  <div class="list">
    {#each options as o (o.kit.id)}
      <button class="opt" disabled={busy || o.holder != null || o.faulty || !store.connected} onclick={() => swapTo(o.kit)} data-testid="swap-option">
        <span class="pill">{KIND_LABEL[o.kit.kind]}</span>
        <strong class="tag">{consoleTag(o.kit)}</strong>
        <span class="name">{o.kit.name}{#if o.kit.is_spare} <span class="pill ok">spare</span>{/if}</span>
        <span class="eq muted">{store.label(o.kit.carrier_id)}{#if o.kit.element_id}{' + '}{store.label(o.kit.element_id)}{/if}</span>
        <BatteryBadge id={o.kit.carrier_id} />
        {#if o.holder != null}<span class="pill warn">in use: {store.performerName(o.holder)}</span>{/if}
        {#if o.faulty}<span class="pill crit">faulty</span>{/if}
      </button>
    {:else}
      <p class="muted">No spare or free {KIND_LABEL[from.kind]} kits in this show. Add spares in Admin → Show &amp; kits.</p>
    {/each}
  </div>
</Modal>

<style>
  .opts button { flex: 1; min-height: var(--touch); }
  .chk { display: flex; align-items: center; gap: 6px; color: var(--text); font-size: 0.95rem; }
  .chk input { min-height: auto; width: 22px; height: 22px; }
  .small { font-size: 0.85rem; margin: 0; }
  .list { display: grid; gap: 8px; }
  .opt { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; min-height: 64px; text-align: left; font-weight: 400; }
  .tag { font-size: 1.15rem; min-width: 60px; }
  .name { font-weight: 650; }
  .eq { flex: 1; }
</style>
