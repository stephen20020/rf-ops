<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import Modal from './Modal.svelte';
  import type { Id, Severity } from '../lib/types';

  let {
    onclose,
    performerId = null,
    equipmentId = null,
  }: { onclose: () => void; performerId?: Id | null; equipmentId?: Id | null } = $props();

  const PRESETS = ['Dropout', 'No signal', 'Low battery', 'Crackle / noise', 'Wrong pack', 'Mic not on', 'IEM no audio'];

  let severity = $state<Severity>('critical');
  let title = $state('');
  let detail = $state('');
  // svelte-ignore state_referenced_locally
  let performer = $state<Id | null>(performerId);
  // svelte-ignore state_referenced_locally
  let equipment = $state<Id | null>(equipmentId);

  const performers = $derived([...store.performers.values()].sort((a, b) => a.name.localeCompare(b.name)));
  const items = $derived(
    performer == null
      ? [...store.equipment.values()].filter((e) => !['receiver', 'battery'].includes(e.kind))
      : store
          .kitsFor(store.show?.effective, performer)
          .flatMap((k) => [k.carrier_id, k.element_id, k.base_id])
          .filter((i): i is Id => i != null)
          .map((i) => store.equipment.get(i)!)
          .filter(Boolean),
  );

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!title.trim()) return;
    const ok = await store.act(() =>
      post('/api/alerts', {
        show_id: store.showId,
        severity,
        title: title.trim(),
        detail,
        performer_id: performer,
        equipment_id: equipment,
      }),
    );
    if (ok) onclose();
  }
</script>

<Modal title="Raise alert" {onclose}>
  <form class="form" onsubmit={submit}>
    <div class="row sev">
      {#each ['critical', 'warning', 'info'] as const as s (s)}
        <button type="button" class="big" class:danger={s === 'critical' && severity === s} class:warn={s === 'warning' && severity === s} class:primary={s === 'info' && severity === s} onclick={() => (severity = s)}>{s}</button>
      {/each}
    </div>
    <div class="row presets">
      {#each PRESETS as p (p)}
        <button type="button" class:primary={title === p} onclick={() => (title = p)}>{p}</button>
      {/each}
    </div>
    <label>Title<input bind:value={title} placeholder="What's wrong?" data-testid="alert-title" /></label>
    <label>Performer
      <select bind:value={performer}>
        <option value={null}>—</option>
        {#each performers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
    </label>
    <label>Equipment
      <select bind:value={equipment}>
        <option value={null}>—</option>
        {#each items as i (i.id)}<option value={i.id}>{i.label} ({i.model || i.kind})</option>{/each}
      </select>
    </label>
    <label>Detail<textarea rows="2" bind:value={detail}></textarea></label>
    <button class="danger big" type="submit" disabled={!title.trim() || !store.connected}>Raise alert</button>
  </form>
</Modal>

<style>
  .form { display: grid; gap: 12px; }
  .sev button { flex: 1; text-transform: capitalize; }
  .presets button { min-height: 44px; }
</style>
