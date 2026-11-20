<script lang="ts">
  // Problems reported on devices (and rule alerts), NEW → ON IT → FIXED.
  import { post } from '../../lib/api';
  import { timeAgo } from '../../lib/format';
  import { store } from '../../lib/store.svelte';
  import type { Alert } from '../../lib/types';

  const open = $derived(store.alerts.filter((a) => a.state !== 'resolved'));
  const isRule = (a: Alert) => a.source === 'rule';
  const shown = $derived(open.filter((a) => !isRule(a) || a.severity === 'critical'));
  const ruleCount = $derived(open.filter((a) => isRule(a) && a.severity !== 'critical').length);
  const onIt = (a: Alert) => store.act(() => post(`/api/alerts/${a.id}/ack`), 'On it');
  const fixed = (a: Alert) => store.act(() => post(`/api/alerts/${a.id}/resolve`, { note: null }), 'Fixed');
</script>

{#if shown.length || ruleCount}
  <h2 class:crit={shown.some((a) => a.severity === 'critical')}>Problems</h2>
  {#each shown as a (a.id)}
    <div class="alert" class:critical={a.severity === 'critical'} class:onit={a.state === 'acknowledged'} data-testid="board-alert">
      <div class="grow">
        <div class="t">{a.severity === 'critical' ? '⬣ ' : '▲ '}{a.title}</div>
        <div class="muted small">
          {a.opened_by} · {timeAgo(a.opened_at, store.now)}{#if a.state === 'acknowledged'} · <b>{a.acked_by} is on it</b>{/if}
        </div>
      </div>
      {#if a.state !== 'acknowledged'}<button class="big" onclick={() => onIt(a)}>On it</button>{/if}
      <button class="big primary" onclick={() => fixed(a)}>Fixed</button>
    </div>
  {/each}
  {#if ruleCount}<div class="muted small">{ruleCount} reminder(s) (battery / MIDI) — see Classic or Admin.</div>{/if}
{/if}

<style>
  .alert { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 10px; border: 2px solid var(--warn); background: var(--panel-2); }
  .alert.critical { border: 4px solid var(--crit); }
  .alert.onit { border-style: dashed; }
  .t { font-weight: 700; }
  .small { font-size: 0.82rem; }
  h2.crit { color: var(--crit); }
</style>
