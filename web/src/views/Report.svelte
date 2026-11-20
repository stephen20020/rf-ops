<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { get } from '../lib/api';
  import type { Report } from '../lib/types';

  let report = $state<Report | null>(null);
  let error = $state('');

  $effect(() => {
    const id = store.showId;
    if (!id) return;
    get<Report>(`/api/shows/${id}/report`)
      .then((r) => (report = r))
      .catch((e) => (error = String(e)));
  });

  const t = (iso: string | null) => (iso ? new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : '');
  const mins = (a: string | null, b: string | null) =>
    a && b ? `${Math.max(0, Math.round((Date.parse(b) - Date.parse(a)) / 1000))} s` : '';
</script>

<article class="report">
  <div class="noprint row">
    <a href="#/admin">← Back</a>
    <span class="grow"></span>
    <button class="primary" onclick={() => window.print()}>Print / save PDF</button>
  </div>
  {#if error}<p>{error}</p>{/if}
  {#if report}
    {@const r = report}
    <header>
      <h1>{r.show.name} — post-show report</h1>
      <p>Generated {new Date(r.generated_at).toLocaleString()}</p>
    </header>
    <section class="summary">
      <div><b>{r.alerts.length}</b> alerts ({r.alerts.filter((a) => a.severity === 'critical').length} critical)</div>
      <div><b>{r.swaps.length}</b> swaps</div>
      <div><b>{r.faulty_items.length}</b> faulty items</div>
      <div><b>{r.battery_checks.length}</b> battery checks</div>
      <div><b>{r.handoffs_done}</b> handoffs ticked</div>
    </section>

    <h2>Faulty equipment — needs attention</h2>
    {#if r.faulty_items.length}
      <table><thead><tr><th>Label</th><th>Type</th><th>Model</th><th>Notes</th></tr></thead>
        <tbody>{#each r.faulty_items as f (f.id)}<tr><td><b>{f.label}</b></td><td>{f.kind}</td><td>{f.model}</td><td>{f.notes}</td></tr>{/each}</tbody></table>
    {:else}<p>None.</p>{/if}

    <h2>Alerts</h2>
    {#if r.alerts.length}
      <table>
        <thead><tr><th>Opened</th><th>Severity</th><th>Alert</th><th>Who / what</th><th>Ack</th><th>Resolved</th><th>Note</th></tr></thead>
        <tbody>
          {#each r.alerts as a (a.id)}
            <tr>
              <td>{t(a.opened_at)}<br /><small>{a.opened_by}</small></td>
              <td>{a.severity}</td>
              <td>{a.title}</td>
              <td>{a.performer_id ? store.performerName(a.performer_id) : ''}{a.equipment_id ? ` ${store.label(a.equipment_id)}` : ''}</td>
              <td>{a.acked_by ?? ''}<br /><small>{mins(a.opened_at, a.acked_at)}</small></td>
              <td>{a.resolved_by ?? (a.state === 'resolved' ? '' : 'OPEN')}<br /><small>{mins(a.opened_at, a.resolved_at)}</small></td>
              <td>{a.resolution_note ?? ''}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}<p>No alerts.</p>{/if}

    <h2>Swaps</h2>
    {#if r.swaps.length}
      <table><thead><tr><th>Time</th><th>Performer</th><th>From</th><th>To</th><th>Scope</th><th>Song</th><th>By</th></tr></thead>
        <tbody>{#each r.swaps as s (s.id)}<tr><td>{t(s.at)}</td><td>{s.performer}</td><td>{s.from_kit ?? ''}</td><td>{s.to_kit}</td><td>{s.scope === 'song' ? 'song only' : 'rest of show'}</td><td>{s.song ?? 'pre-show'}</td><td>{s.by}</td></tr>{/each}</tbody></table>
    {:else}<p>No swaps.</p>{/if}

    <h2>Song changes</h2>
    {#if r.song_changes.length}
      <table><thead><tr><th>Time</th><th>Song</th><th>Source</th><th>By</th></tr></thead>
        <tbody>{#each r.song_changes as c, i (i)}<tr><td>{t(c.at)}</td><td>{c.song ?? 'pre-show'}</td><td>{c.source ?? ''}</td><td>{c.actor}</td></tr>{/each}</tbody></table>
    {:else}<p>None recorded.</p>{/if}

    <h2>Battery checks</h2>
    {#if r.battery_checks.length}
      <table><thead><tr><th>Time</th><th>Pack</th><th>Result</th><th>Battery</th><th>By</th></tr></thead>
        <tbody>{#each r.battery_checks as b, i (i)}<tr><td>{t(b.checked_at)}</td><td>{b.label}</td><td>{b.result}</td><td>{b.battery_type}</td><td>{b.checked_by}</td></tr>{/each}</tbody></table>
    {:else}<p>None since call time.</p>{/if}
  {:else if !error}
    <p>Loading…</p>
  {/if}
</article>

<style>
  .report { max-width: 1000px; margin: 0 auto; padding: 16px; display: grid; gap: 12px; }
  .summary { display: flex; gap: 20px; flex-wrap: wrap; }
  h2 { margin-top: 12px; }
  td, th { font-size: 0.9rem; }
  small { color: var(--muted); }
  @media print {
    :global(body) { background: #fff; color: #000; }
    .noprint { display: none; }
    .report { max-width: none; padding: 0; }
    td, th { border-color: #999; color: #000; }
    small { color: #444; }
  }
</style>
