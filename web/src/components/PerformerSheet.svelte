<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { consoleTag, DUE_LABEL, KIND_LABEL, timeAgo } from '../lib/format';
  import Modal from './Modal.svelte';
  import RaiseAlert from './RaiseAlert.svelte';
  import AlertList from './AlertList.svelte';
  import type { Id, Performer } from '../lib/types';

  let { performer, onclose }: { performer: Performer; onclose: () => void } = $props();

  let raising = $state<{ equipment: Id | null } | null>(null);

  const now = $derived(store.kitsFor(store.show?.effective, performer.id));
  const next = $derived(store.kitsFor(store.show?.next_effective, performer.id));
  const kits = $derived([...new Map([...now, ...next].map((k) => [k.id, k])).values()]);
  const alerts = $derived(store.alertsFor(performer.id, kits));

  function check(equipmentId: Id, result: 'fresh' | 'ok' | 'replaced' | 'low') {
    return store.act(() => post('/api/battery-checks', { equipment_id: equipmentId, result }), `Battery ${result}: ${store.label(equipmentId)}`);
  }
</script>

{#if raising}
  <RaiseAlert performerId={performer.id} equipmentId={raising.equipment} onclose={() => (raising = null)} />
{:else}
  <Modal title={performer.name} {onclose}>
    <div class="muted">{performer.role}{performer.zone ? ` · ${performer.zone}` : ''}{performer.owner_user_id ? ` · A2: ${store.users.get(performer.owner_user_id)?.name ?? ''}` : ''}</div>
    {#if alerts.length}<AlertList {alerts} compact />{/if}
    {#each kits as k (k.id)}
      {@const b = k.carrier_id != null ? store.battery.get(k.carrier_id) : undefined}
      <div class="kit panel">
        <div class="row">
          <span class="pill">{KIND_LABEL[k.kind]}</span>
          <strong class="grow">{k.name}</strong>
          <span>{consoleTag(k)}</span>
          {#if !now.some((n) => n.id === k.id)}<span class="pill warn">next song</span>{/if}
        </div>
        <div class="muted">
          {store.label(k.base_id)} · {store.label(k.carrier_id)}{#if k.element_id}{' + '}{store.label(k.element_id)}{/if}
        </div>
        {#if b && k.carrier_id != null}
          {@const carrier = k.carrier_id}
          <div class="row">
            <span class="grow">🔋 {DUE_LABEL[b.due]}{#if b.last_checked_at} · {b.last_result} by {b.last_checked_by}, {timeAgo(b.last_checked_at, store.now)}{/if}</span>
          </div>
          <div class="row checks">
            <button class="big" onclick={() => check(carrier, 'fresh')}>Fresh</button>
            <button class="big" onclick={() => check(carrier, 'ok')}>Checked OK</button>
            <button class="big" onclick={() => check(carrier, 'replaced')}>Replaced</button>
            <button class="big warn" onclick={() => check(carrier, 'low')}>Low</button>
          </div>
        {/if}
        <button class="danger" onclick={() => (raising = { equipment: k.carrier_id })}>Raise alert on {store.label(k.carrier_id)}</button>
      </div>
    {:else}
      <p class="muted">No kit in this or the next song.</p>
    {/each}
    <button class="danger big" onclick={() => (raising = { equipment: null })}>Raise alert for {performer.name}</button>
  </Modal>
{/if}

<style>
  .kit { display: grid; gap: 8px; background: var(--panel-2); }
  .checks button { flex: 1; min-width: 110px; }
</style>
