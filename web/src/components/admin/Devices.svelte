<script lang="ts">
  // Devices shared by every show (data model §2.2): the board's tiles.
  import { get, patch, post } from '../../lib/api';
  import { plan } from '../../lib/plan.svelte';
  import { store } from '../../lib/store.svelte';
  import type { Id } from '../../lib/types';

  type Dev = { id: Id; label: string; kind: string; channel: string | null; is_spare: boolean; status: string };
  let devs = $state<Dev[]>([]);
  let nf = $state({ label: '', kind: 'mic', channel: '', base: '', spare: false });

  async function load() {
    devs = await get<Dev[]>('/api/devices');
  }
  $effect(() => {
    void plan.data;
    void load();
  });

  async function add(e: SubmitEvent) {
    e.preventDefault();
    const ok = await store.act(
      () => post('/api/devices/new', { label: nf.label, kind: nf.kind, channel: nf.channel || null, base: nf.base || null, spare: nf.spare }),
      `Added ${nf.label}`,
    );
    if (ok) nf.label = '';
  }
  const upd = (id: Id, body: Record<string, unknown>) => store.act(() => patch(`/api/devices/${id}`, body));
  const setStatus = (id: Id, status: string) => store.act(() => post(`/api/devices/${id}/status`, { status }));
</script>

<section class="panel grid">
  <h2>Devices</h2>
  <p class="muted small">Every mic, IEM pack and guitar pack, shared by all shows. "Base" is the inventory label of its receiver channel or IEM transmitter (e.g. TX1), used for mixes and retunes.</p>
  <form class="row" onsubmit={add}>
    <input placeholder="Label (Red, RX11…)" bind:value={nf.label} aria-label="Device label" />
    <select bind:value={nf.kind} aria-label="Kind"><option value="mic">mic</option><option value="iem">IEM pack</option><option value="guitar">guitar</option></select>
    <input placeholder="Console ch" bind:value={nf.channel} size="6" aria-label="Console channel" />
    <input placeholder="Base (TX1 / ULXD-1/2)" bind:value={nf.base} aria-label="Base" />
    <label class="row"><input type="checkbox" bind:checked={nf.spare} /> spare</label>
    <button class="primary" disabled={!nf.label.trim()}>Add</button>
  </form>
  <div class="table-wrap">
    <table>
      <thead><tr><th>Label</th><th>Kind</th><th>Console ch</th><th>Spare</th><th>Status</th><th></th></tr></thead>
      <tbody>
        {#each devs as d (d.id)}
          <tr>
            <td><input value={d.label} onchange={(e) => upd(d.id, { label: (e.currentTarget as HTMLInputElement).value })} aria-label="Label" /></td>
            <td>{d.kind}</td>
            <td><input value={d.channel ?? ''} size="5" onchange={(e) => upd(d.id, { channel: (e.currentTarget as HTMLInputElement).value })} aria-label="Channel" /></td>
            <td><input type="checkbox" checked={d.is_spare} onchange={(e) => upd(d.id, { spare: (e.currentTarget as HTMLInputElement).checked })} aria-label="Spare" /></td>
            <td>
              <select value={d.status} onchange={(e) => setStatus(d.id, (e.currentTarget as HTMLSelectElement).value)} aria-label="Status">
                <option value="ok">ok</option><option value="suspect">suspect</option><option value="out">out of service</option><option value="missing">missing</option>
              </select>
            </td>
            <td><button class="ghost" onclick={() => confirm(`Remove ${d.label} from the board?`) && upd(d.id, { active: false })}>Remove</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<style>
  .grid { display: grid; gap: 8px; }
  .small { font-size: 0.85rem; }
</style>
