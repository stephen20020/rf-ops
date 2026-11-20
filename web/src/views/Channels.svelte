<script lang="ts">
  // FOH (interface spec §14.3): console channels with who is on them now, swap cards to confirm
  // at the console (D2), IEM packs with their mix.
  import SongBar from '../components/SongBar.svelte';
  import SwapCard from '../components/board/SwapCard.svelte';
  import { get } from '../lib/api';
  import { plan, type Tile } from '../lib/plan.svelte';
  import { store } from '../lib/store.svelte';
  import type { Id } from '../lib/types';

  type Dev = { id: Id; label: string; kind: string; channel: string | null; is_spare: boolean; status: string };
  let devs = $state<Dev[]>([]);

  $effect(() => {
    if (store.showId) void plan.load();
  });
  $effect(() => {
    void plan.data; // reload channels with the board
    get<Dev[]>('/api/devices').then((d) => (devs = d)).catch(() => {});
  });

  const tile = (id: Id): Tile | undefined => plan.data?.devices.find((t) => t.id === id);
  const toConfirm = $derived(plan.swaps.filter((s) => s.state === 'in_hand'));
  const pending = $derived(plan.swaps.filter((s) => s.state === 'recorded' || s.state === 'return_recorded'));
  const chNum = (c: string | null) => (c && /^\d+$/.test(c) ? Number(c) : 9999);
  const mics = $derived(devs.filter((d) => d.kind !== 'iem').sort((a, b) => chNum(a.channel) - chNum(b.channel) || a.label.localeCompare(b.label)));
  const iems = $derived(devs.filter((d) => d.kind === 'iem'));
  function who(t: Tile | undefined): string {
    if (!t) return '';
    const u = t.usage;
    if (u.usage === 'in_use') return plan.person(u.by);
    if (u.usage === 'on_spare_for') return t.holder.kind === 'person' ? plan.person(t.holder.id) : '';
    if (u.usage === 'pulled') return 'PULLED — mute';
    if (u.usage === 'next') return `(next: ${plan.person(u.by)})`;
    return '';
  }
</script>

<div class="foh">
  <SongBar compact />

  {#if toConfirm.length}
    <section class="panel urgent">
      <h2>Confirm at the console</h2>
      {#each toConfirm as s (s.id)}<SwapCard {s} foh />{/each}
    </section>
  {/if}
  {#if pending.length}
    <section class="panel">
      <h2>Coming from backstage</h2>
      <p class="muted small">Wait for <b>In hand</b> before unmuting.</p>
      {#each pending as s (s.id)}<SwapCard {s} foh />{/each}
    </section>
  {/if}

  <section class="panel">
    <h2>Mic channels</h2>
    <table>
      <thead><tr><th>Ch</th><th>Device</th><th>On it now</th><th></th></tr></thead>
      <tbody>
        {#each mics as d (d.id)}
          {@const t = tile(d.id)}
          <tr data-testid="foh-row" class:off={!who(t)} class:bad={t?.usage.usage === 'pulled' || t?.usage.usage === 'out' || t?.usage.usage === 'suspect'}>
            <td class="ch">{d.channel ?? '—'}</td>
            <td>{d.label}{d.is_spare ? ' (spare)' : ''}</td>
            <td><b>{who(t)}</b></td>
            <td class="muted small">{t?.usage.usage === 'on_spare_for' ? `spare for ${plan.device(t.usage.failed)}` : t?.usage.usage === 'suspect' ? 'suspect' : ''}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>

  {#if iems.length}
    <section class="panel">
      <h2>IEM packs</h2>
      <table>
        <thead><tr><th>Pack</th><th>Mix</th><th>On it now</th></tr></thead>
        <tbody>
          {#each iems as d (d.id)}
            {@const t = tile(d.id)}
            <tr class:off={!who(t)}><td>{d.label}</td><td>{t?.mix ?? '—'}</td><td><b>{who(t)}</b></td></tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/if}
</div>

<style>
  .foh { display: grid; gap: 12px; }
  .urgent { border: 2px solid var(--accent); }
  .ch { font-variant-numeric: tabular-nums; font-weight: 700; width: 4em; }
  tr.off td { color: var(--muted); }
  tr.bad td { color: var(--crit); }
  .small { font-size: 0.85rem; }
  section { display: grid; gap: 8px; }
</style>
