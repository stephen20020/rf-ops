<script lang="ts">
  // The mic plot (interface spec §15.1): one row per device, Default + one column per song.
  // Editable any time, from any screen (R7). Song cells show the Default greyed until set.
  import { del, get, post, put } from '../../lib/api';
  import { plan } from '../../lib/plan.svelte';
  import { store } from '../../lib/store.svelte';
  import type { Id } from '../../lib/types';

  type Cell = { device: Id; song: Id | null; performer: Id | null };
  type Pass = { id: Id; device: Id; song: Id; seq: number; to: Id | null };
  let cells = $state<Cell[]>([]);
  let passes = $state<Pass[]>([]);
  let kind = $state<'all' | 'mic' | 'iem' | 'guitar'>('all');

  async function load() {
    if (!store.showId) return;
    const r = await get<{ cells: Cell[]; passes: Pass[] }>(`/api/shows/${store.showId}/plot`);
    cells = r.cells;
    passes = r.passes;
  }
  $effect(() => {
    void plan.data;
    void load();
  });
  $effect(() => {
    if (store.showId) void plan.load();
  });

  const devices = $derived((plan.data?.devices ?? []).filter((d) => kind === 'all' || d.kind === kind));
  const songs = $derived(store.songs);
  const people = $derived([...(store.snapshot?.performers ?? [])].sort((a, b) => a.name.localeCompare(b.name)));
  const key = (d: Id, s: Id | null) => `${d}:${s ?? 0}`;
  const byKey = $derived(new Map(cells.map((c) => [key(c.device, c.song), c])));

  function value(d: Id, s: Id | null): string {
    const c = byKey.get(key(d, s));
    if (!c) return s == null ? 'nobody' : 'inherit';
    return c.performer == null ? 'nobody' : String(c.performer);
  }
  function inherited(d: Id): string {
    const c = byKey.get(key(d, null));
    return c?.performer != null ? plan.person(c.performer) : '';
  }
  async function set(d: Id, s: Id | null, v: string) {
    const body = v === 'inherit' ? { device: d, song: s, performer: null, clear: true } : { device: d, song: s, performer: v === 'nobody' || v === '' ? null : Number(v) };
    await store.act(() => put(`/api/shows/${store.showId}/plot`, body));
  }

  // Passes
  let pSong = $state<Id | null>(null);
  let pDevice = $state<Id | null>(null);
  let pTo = $state<Id | null>(null);
  async function addPass() {
    if (pSong == null || pDevice == null) return;
    await store.act(() => post(`/api/shows/${store.showId}/passes`, { device: pDevice, song: pSong, to: pTo }), 'Pass added');
  }
  const removePass = (id: Id) => store.act(() => del(`/api/passes/${id}`), 'Pass removed');
</script>

<section class="panel grid">
  <div class="row">
    <h2 class="grow">Plot — {store.show?.show.name ?? ''}</h2>
    {#each [['all', 'All'], ['mic', 'Mics'], ['iem', 'IEM'], ['guitar', 'Guitar']] as [k, n] (k)}
      <button class:primary={kind === k} onclick={() => (kind = k as typeof kind)}>{n}</button>
    {/each}
  </div>
  <p class="muted small">Pick who has each device. Song columns only need the differences: "same as default" follows the Default column. Changes apply straight away on every screen.</p>
  <div class="table-wrap">
    <table class="plot" data-testid="plot-grid">
      <thead>
        <tr><th>Device</th><th>Default</th>{#each songs as s (s.id)}<th title={s.title}>{s.title}</th>{/each}</tr>
      </thead>
      <tbody>
        {#each devices as d (d.id)}
          <tr>
            <td class="dev"><b>{d.label}</b>{d.mix ? ` · ${d.mix}` : ''}</td>
            <td>
              <select value={value(d.id, null)} onchange={(e) => set(d.id, null, (e.currentTarget as HTMLSelectElement).value)} aria-label={`${d.label} default`}>
                <option value="nobody">nobody</option>
                {#each people as p (p.id)}<option value={String(p.id)}>{p.name}</option>{/each}
              </select>
            </td>
            {#each songs as s (s.id)}
              {@const v = value(d.id, s.id)}
              <td class:inherit={v === 'inherit'}>
                <select value={v} onchange={(e) => set(d.id, s.id, (e.currentTarget as HTMLSelectElement).value)} aria-label={`${d.label} in ${s.title}`}>
                  <option value="inherit">{inherited(d.id) ? `(${inherited(d.id)})` : '(same)'}</option>
                  <option value="nobody">nobody</option>
                  {#each people as p (p.id)}<option value={String(p.id)}>{p.name}</option>{/each}
                </select>
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<section class="panel grid">
  <h2>Passes during a song</h2>
  <p class="muted small">A hand-over in the middle of a song (e.g. RX10 Helen → Shosh). The return afterwards is automatic.</p>
  {#each passes as p (p.id)}
    <div class="row"><span class="grow">{plan.song(p.song)}: {plan.device(p.device)} → {plan.person(p.to)}</span><button class="ghost" onclick={() => removePass(p.id)}>Remove</button></div>
  {:else}<p class="muted">No passes.</p>{/each}
  <div class="row">
    <select bind:value={pSong} aria-label="Pass song"><option value={null}>Song…</option>{#each songs as s (s.id)}<option value={s.id}>{s.title}</option>{/each}</select>
    <select bind:value={pDevice} aria-label="Pass device"><option value={null}>Device…</option>{#each plan.data?.devices ?? [] as d (d.id)}<option value={d.id}>{d.label}</option>{/each}</select>
    <select bind:value={pTo} aria-label="Pass to"><option value={null}>to nobody</option>{#each people as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
    <button onclick={addPass} disabled={pSong == null || pDevice == null}>Add pass</button>
  </div>
</section>

<style>
  .grid { display: grid; gap: 8px; }
  .small { font-size: 0.85rem; }
  .plot select { min-width: 110px; }
  .plot td.inherit select { color: var(--muted); }
  .plot th { white-space: nowrap; max-width: 160px; overflow: hidden; text-overflow: ellipsis; }
  .dev { white-space: nowrap; position: sticky; left: 0; background: var(--panel); }
</style>
