<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { del, patch, post } from '../../lib/api';
  import type { EquipmentKind, EquipmentStatus } from '../../lib/types';

  const KINDS: EquipmentKind[] = [
    'receiver', 'receiver_channel', 'handheld_tx', 'bodypack_tx', 'capsule', 'headset', 'lav',
    'iem_tx', 'iem_tx_channel', 'iem_pack', 'guitar_receiver', 'guitar_pack', 'battery',
  ];
  const STATUSES: EquipmentStatus[] = ['available', 'checked_out', 'in_use', 'faulty', 'retired'];

  let kindFilter = $state<EquipmentKind | ''>('');
  let q = $state('');
  let form = $state({ kind: 'handheld_tx' as EquipmentKind, make: '', model: '', label: '', serial: '', parent_id: null as number | null, channel_no: '', ip_address: '' });

  const list = $derived(
    [...store.equipment.values()]
      .filter((e) => !kindFilter || e.kind === kindFilter)
      .filter((e) => !q || `${e.label} ${e.make} ${e.model} ${e.serial ?? ''}`.toLowerCase().includes(q.toLowerCase())),
  );
  const parents = $derived([...store.equipment.values()].filter((e) => e.kind === 'receiver' || e.kind === 'iem_tx'));

  async function add(e: SubmitEvent) {
    e.preventDefault();
    const ok = await store.act(() =>
      post('/api/equipment', {
        ...form,
        serial: form.serial || null,
        channel_no: form.channel_no ? Number(form.channel_no) : null,
        ip_address: form.ip_address || null,
      }),
      `Added ${form.label}`,
    );
    if (ok) form.label = '';
  }
</script>

<section class="panel grid">
  <h2>Add equipment</h2>
  <form class="row" onsubmit={add}>
    <label>Kind<select bind:value={form.kind}>{#each KINDS as k (k)}<option value={k}>{k}</option>{/each}</select></label>
    <label>Make<input bind:value={form.make} placeholder="Shure" /></label>
    <label>Model<input bind:value={form.model} placeholder="ULXD2" /></label>
    <label>Label *<input bind:value={form.label} placeholder="HH9" required /></label>
    <label>Serial<input bind:value={form.serial} /></label>
    {#if form.kind === 'receiver_channel' || form.kind === 'iem_tx_channel'}
      <label>Parent<select bind:value={form.parent_id}><option value={null}>—</option>{#each parents as p (p.id)}<option value={p.id}>{p.label}</option>{/each}</select></label>
      <label>Channel<input type="number" min="1" bind:value={form.channel_no} /></label>
    {/if}
    {#if ['receiver', 'iem_tx', 'iem_tx_channel'].includes(form.kind)}
      <label>IP<input bind:value={form.ip_address} placeholder="192.168.1.21" /></label>
    {/if}
    <button class="primary" type="submit" disabled={!form.label.trim()}>Add</button>
  </form>
</section>

<section class="panel grid">
  <div class="row">
    <h2 class="grow">Inventory · {list.length}</h2>
    <input placeholder="Search" bind:value={q} aria-label="Search equipment" />
    <select bind:value={kindFilter} aria-label="Filter by kind"><option value="">All kinds</option>{#each KINDS as k (k)}<option value={k}>{k}</option>{/each}</select>
  </div>
  <div class="table-wrap">
    <table>
      <thead><tr><th>Label</th><th>Kind</th><th>Make / model</th><th>Serial</th><th>Parent / IP</th><th>Status</th><th></th></tr></thead>
      <tbody>
        {#each list as e (e.id)}
          <tr>
            <td><strong>{e.label}</strong></td>
            <td>{e.kind}</td>
            <td>{e.make} {e.model}</td>
            <td>{e.serial ?? ''}</td>
            <td>{e.parent_id ? `${store.label(e.parent_id)}${e.channel_no ? ` ch ${e.channel_no}` : ''}` : (e.ip_address ?? '')}</td>
            <td>
              <select value={e.status} onchange={(ev) => store.act(() => patch(`/api/equipment/${e.id}`, { status: (ev.currentTarget as HTMLSelectElement).value }))} aria-label="Status of {e.label}">
                {#each STATUSES as s (s)}<option value={s}>{s}</option>{/each}
              </select>
            </td>
            <td><button class="ghost" onclick={() => confirm(`Delete ${e.label}?`) && store.act(() => del(`/api/equipment/${e.id}`))}>Delete</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<style>
  .grid { display: grid; gap: 10px; }
  form label { min-width: 120px; }
</style>
