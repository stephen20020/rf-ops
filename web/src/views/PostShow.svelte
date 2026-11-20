<script lang="ts">
  // Post-show report (interface spec §16): what happened, what needs attention, for printing.
  import { get } from '../lib/api';
  import { store } from '../lib/store.svelte';

  type R = any; // shape from /api/shows/{id}/post-show
  let r = $state<R | null>(null);
  $effect(() => {
    void store.lastSeq;
    if (store.showId) get<R>(`/api/shows/${store.showId}/post-show`).then((x) => (r = x)).catch(() => {});
  });
  const t = (iso: string | null | undefined) => (iso ? new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '');
  const kindText = (k: string) => ({ swap: 'Swap', pull: 'Pull', reassign: 'Reassign', guest: 'Guest', tx_swap: 'TX swap' })[k] ?? k;
</script>

{#if r}
  <article class="report" data-testid="post-show">
    <div class="row noprint"><button class="primary" onclick={() => window.print()}>Print</button></div>
    <h1>{r.show.name} — post-show report</h1>
    <p class="muted">
      {[r.show.day, r.show.set].filter(Boolean).join(' ')}
      {r.show.started_at ? ` · started ${t(r.show.started_at)}` : ' · not started yet'}{r.show.ended_at ? ` · ended ${t(r.show.ended_at)}` : ''}
      · {r.handoffs_done} handoffs done{r.handoffs_left ? `, ${r.handoffs_left} left` : ''}
    </p>

    <section>
      <h2>Needs attention</h2>
      {#if !r.attention.length && !r.still_out.length}<p>Nothing — all devices back and OK.</p>{/if}
      <ul>
        {#each r.attention as a, i (i)}<li><b>{a.device}</b>: {a.status}{a.with ? ` (with ${a.with})` : ''}</li>{/each}
        {#each r.still_out as o, i (i)}<li><b>{o.device}</b> not returned — last with {o.with ?? '?'}</li>{/each}
      </ul>
    </section>

    <section>
      <h2>Swaps ({r.swaps.length})</h2>
      {#if r.swaps.length}
        <table>
          <thead><tr><th>What</th><th>Who</th><th>Failed part</th><th>Steps</th><th>State</th></tr></thead>
          <tbody>
            {#each r.swaps as s, i (i)}
              <tr>
                <td>{kindText(s.kind)}: {s.failed ?? ''}{s.replacement ? ` → ${s.replacement}` : ''}</td>
                <td>{s.performer ?? 'everyone'}</td>
                <td>{s.component}</td>
                <td>{#each s.timeline as st, j (j)}<span>{st.step} {t(st.at)} {st.by}{j < s.timeline.length - 1 ? ' · ' : ''}</span>{/each}</td>
                <td>{s.state}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}<p class="muted">No swaps.</p>{/if}
    </section>

    <section>
      <h2>Problems ({r.problems.length})</h2>
      {#if r.problems.length}
        <table>
          <thead><tr><th></th><th>Problem</th><th>Reported</th><th>On it</th><th>Fixed</th><th>Note</th></tr></thead>
          <tbody>
            {#each r.problems as p, i (i)}
              <tr>
                <td>{p.severity === 'critical' ? '⬣' : '▲'}</td><td>{p.title}</td><td>{t(p.opened_at)} {p.opened_by}</td>
                <td>{t(p.acked_at)} {p.acked_by ?? ''}</td><td>{t(p.resolved_at)} {p.resolved_by ?? (p.state === 'resolved' ? '' : 'open')}</td><td>{p.note ?? ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}<p class="muted">No problems reported.</p>{/if}
    </section>

    <section>
      <h2>Batteries</h2>
      <p>
        {Object.entries(r.batteries.counts).map(([k, v]) => `${v} ${k}`).join(' · ') || 'No checks recorded for this show.'}
        {#if r.batteries.low.length} — LOW: {r.batteries.low.map((l: any) => `${l.device} (${t(l.at)})`).join(', ')}{/if}
      </p>
    </section>

    <section>
      <h2>Songs</h2>
      <ol>{#each r.played as p, i (i)}<li>{p.song} <span class="muted">{t(p.at)} · {p.via}</span></li>{/each}</ol>
      {#if r.not_played.length}<p class="muted">Not played: {r.not_played.join(', ')}</p>{/if}
    </section>

    {#if r.changes.length}
      <section>
        <h2>Changes during the show ({r.changes.length})</h2>
        <ul>{#each r.changes as c, i (i)}<li>{t(c.at)} {c.by}: {c.type.replace('.', ' ')}</li>{/each}</ul>
      </section>
    {/if}
  </article>
{:else}
  <p class="muted">Loading the report…</p>
{/if}

<style>
  .report { display: grid; gap: 14px; max-width: 1100px; }
  section { display: grid; gap: 6px; }
  ul, ol { margin: 0; }
  @media print {
    .noprint { display: none; }
    :global(header) { display: none !important; }
    .report { color: #000; }
    :global(body) { background: #fff; }
  }
</style>
