<script lang="ts">
  // Paper plot (the fallback when RF Ops is offline): who has each device in each song, passes.
  import { get } from '../lib/api';
  import { plan } from '../lib/plan.svelte';
  import { store } from '../lib/store.svelte';
  import type { Id } from '../lib/types';

  type Cell = { device: Id; song: Id | null; performer: Id | null };
  type Pass = { id: Id; device: Id; song: Id; seq: number; to: Id | null };
  let cells = $state<Cell[]>([]);
  let passes = $state<Pass[]>([]);
  $effect(() => {
    if (!store.showId) return;
    void plan.load();
    get<{ cells: Cell[]; passes: Pass[] }>(`/api/shows/${store.showId}/plot`).then((r) => {
      cells = r.cells;
      passes = r.passes;
    });
  });
  const songs = $derived(store.songs.filter((s) => !s.skipped));
  const byKey = $derived(new Map(cells.map((c) => [`${c.device}:${c.song ?? 0}`, c.performer])));
  function who(d: Id, s: Id): string {
    const k = `${d}:${s}`;
    const p = byKey.has(k) ? byKey.get(k) : byKey.get(`${d}:0`);
    const base = p == null ? '' : plan.person(p);
    const ps = passes.filter((x) => x.device === d && x.song === s).map((x) => `→ ${plan.person(x.to)}`);
    return [base, ...ps].filter(Boolean).join(' ');
  }
  const devices = $derived((plan.data?.devices ?? []).filter((t) => songs.some((s) => who(t.id, s.id))));
</script>

<div class="print">
  <div class="row noprint"><button class="primary" onclick={() => window.print()}>Print</button><a href="#/checklist">Back</a></div>
  <h1>{store.show?.show.name ?? ''} — paper plot</h1>
  <p class="small">Printed {new Date().toLocaleString()}. "A → B" = pass during the song. Use this if RF Ops is offline.</p>
  <table>
    <thead><tr><th>Device</th>{#each songs as s, i (s.id)}<th>{i + 1}. {s.title}</th>{/each}</tr></thead>
    <tbody>
      {#each devices as d (d.id)}
        <tr><td><b>{d.label}</b>{d.mix ? ` · ${d.mix}` : ''}</td>{#each songs as s (s.id)}<td>{who(d.id, s.id)}</td>{/each}</tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .print { background: #fff; color: #000; padding: 16px; display: grid; gap: 8px; }
  .print h1 { color: #000; }
  table { border-collapse: collapse; width: 100%; font-size: 11px; }
  th, td { border: 1px solid #999; padding: 3px 5px; color: #000; text-transform: none; letter-spacing: 0; }
  th { background: #eee; }
  .small { font-size: 12px; }
  @media print {
    .noprint { display: none; }
    :global(header), :global(.offline) { display: none !important; }
    @page { size: landscape; margin: 10mm; }
  }
</style>
