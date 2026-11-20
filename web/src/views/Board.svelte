<script lang="ts">
  // Backstage wall and A2 phones (interface spec §14.1 / §14.2), on the v3.1 core rules.
  import SongBar from '../components/SongBar.svelte';
  import DeviceTile from '../components/board/DeviceTile.svelte';
  import DevicePanel from '../components/board/DevicePanel.svelte';
  import HandoffCard from '../components/board/HandoffCard.svelte';
  import SwapCard from '../components/board/SwapCard.svelte';
  import UndoBar from '../components/board/UndoBar.svelte';
  import BoardAlerts from '../components/board/BoardAlerts.svelte';
  import { plan, type Tile } from '../lib/plan.svelte';
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';

  type Filter = 'all' | 'mic' | 'iem' | 'guitar';
  let filter = $state<Filter>('all');
  /** Battery round: tap OK / Fresh / LOW straight on each tile. */
  let batteryRound = $state(false);
  let open = $state<Tile | null>(null);

  $effect(() => {
    if (store.showId) void plan.load();
  });

  const b = $derived(plan.data?.board);
  const tiles = $derived((plan.data?.devices ?? []).filter((t) => filter === 'all' || t.kind === filter));
  const groups = $derived(
    (['mic', 'iem', 'guitar'] as const)
      .map((k) => ({ k, name: k === 'mic' ? 'Mics' : k === 'iem' ? 'IEM packs' : 'Guitar', list: tiles.filter((t) => t.kind === k) }))
      .filter((g) => g.list.length > 0),
  );
  const swaps = $derived(plan.swaps.filter((s) => s.active && s.state !== 'reversed' && s.state !== 'returned'));
  const passesNow = $derived((b?.passes ?? []).filter((p) => !p.prep));
  const passesPrep = $derived((b?.passes ?? []).filter((p) => p.prep));
  const nextTitle = $derived(b?.coming_up[0]?.song != null ? plan.song(b.coming_up[0].song) : b?.current_song == null ? 'the first song' : 'after the show');
  const atEnd = $derived(b?.current_song != null && (b?.coming_up ?? []).some((h) => h.song == null));
  const openTile = $derived(open ? (plan.data?.devices.find((t) => t.id === open!.id) ?? open) : null);

  function closeCountIn() {
    if (!confirm('Close count-in? Anything still out is marked MISSING with its last holder.')) return;
    return store.act(() => post(`/api/shows/${store.showId}/count-in/close`), 'Count-in closed');
  }
</script>

<div class="board">
  <div class="songs"><SongBar /></div>

  <aside class="jobs">
    <BoardAlerts />
    {#if swaps.length}
      <h2>Swaps</h2>
      {#each swaps as s (s.id)}<SwapCard {s} />{/each}
    {/if}

    {#if b && b.now.length}
      <h2 class="overdue">Overdue now</h2>
      {#each b.now as h (h.device + JSON.stringify(h.action))}<HandoffCard {h} overdue />{/each}
    {/if}

    {#if passesNow.length}
      <h2>During this song</h2>
      {#each passesNow as p (p.pass)}
        <div class="card" data-testid="pass-card">
          <div class="grow"><b>Pass {plan.device(p.device)}: {plan.person(p.from)} → {plan.person(p.to)}</b><div class="muted small">When it happens in the song</div></div>
          <button class="primary done" onclick={() => plan.passDone(p)}>Pass done</button>
        </div>
      {/each}
    {/if}

    <h2>Before {nextTitle}</h2>
    {#each b?.coming_up ?? [] as h (h.device + JSON.stringify(h.action))}<HandoffCard {h} />{:else}<p class="muted">Nothing to hand over.</p>{/each}
    {#each passesPrep as p (p.pass)}
      <div class="muted small prep">Get ready: {plan.device(p.device)} {plan.person(p.from)} → {plan.person(p.to)} during {nextTitle}</div>
    {/each}

    {#if b && b.conflicts.length}
      <h2 class="overdue">Check the plan</h2>
      {#each b.conflicts as c, i (i)}<div class="conflict" data-testid="conflict">{plan.conflictText(c)}</div>{/each}
    {/if}

    {#if atEnd && (plan.data?.count_in.length ?? 0) > 0}
      <h2>Count-in</h2>
      {#each plan.data?.count_in ?? [] as [d, h] (d)}
        <div class="muted">{plan.device(d)} — {h.kind === 'person' ? plan.person(h.id) : ''}</div>
      {/each}
      <button onclick={closeCountIn}>Close count-in</button>
    {/if}
  </aside>

  <section class="tiles">
    <div class="row filters">
      {#each [['all', 'All'], ['mic', 'Mics'], ['iem', 'IEM'], ['guitar', 'Guitar']] as [k, n] (k)}
        <button class:primary={filter === k} onclick={() => (filter = k as Filter)}>{n}</button>
      {/each}
      <button class:primary={batteryRound} onclick={() => (batteryRound = !batteryRound)} data-testid="battery-round">🔋 Battery round</button>
      {#if batteryRound}
        <span class="muted">{tiles.filter((t) => plan.batteries[String(t.id)]).length} / {tiles.length} checked for this show</span>
      {/if}
    </div>
    {#if !plan.data}
      <p class="muted">Loading the board…</p>
    {:else if !plan.data.devices.length}
      <p class="muted">No devices yet. Import a show file (Admin → Show file) or add devices in Admin.</p>
    {/if}
    {#each groups as g (g.k)}
      <h2>{g.name}</h2>
      <div class="grid">
        {#each g.list as t (t.id)}
          {#if batteryRound}
            {@const bat = plan.batteries[String(t.id)]}
            <div class="battile" class:checked={!!bat} data-testid="battery-tile" data-device={t.label}>
              <b>{t.label}</b>
              <span class="muted small">{bat ? `✓ ${bat.result} · ${bat.by}` : 'not checked'}</span>
              <div class="row">
                <button onclick={() => plan.battery(t.id, 'ok')}>OK</button>
                <button onclick={() => plan.battery(t.id, 'fresh')}>Fresh</button>
                <button class="warn" onclick={() => plan.battery(t.id, 'low')}>LOW</button>
              </div>
            </div>
          {:else}
            <DeviceTile tile={t} onopen={(x) => (open = x)} />
          {/if}
        {/each}
      </div>
    {/each}
  </section>
</div>

{#if openTile}<DevicePanel tile={openTile} onclose={() => (open = null)} />{/if}
<UndoBar />

<style>
  .board { display: grid; grid-template-columns: minmax(0, 1fr) minmax(300px, 420px); grid-template-areas: 'songs songs' 'tiles jobs'; gap: 12px; }
  .songs { grid-area: songs; }
  .tiles { grid-area: tiles; display: grid; gap: 8px; align-content: start; }
  .jobs { grid-area: jobs; display: grid; gap: 8px; align-content: start; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  h2 { margin-top: 6px; }
  h2.overdue { color: var(--warn); }
  .card { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 10px; background: var(--panel-2); border: 2px solid var(--accent); }
  .done { min-height: 64px; min-width: 88px; }
  .small { font-size: 0.82rem; }
  .prep { padding-left: 4px; }
  .battile { display: grid; gap: 4px; padding: 8px; border-radius: 10px; border: 1px solid var(--warn); background: var(--panel); }
  .battile.checked { border-color: var(--line); }
  .battile button { min-height: 44px; flex: 1; padding: 0 6px; }
  .conflict { padding: 8px 10px; border-radius: 8px; border: 1px solid var(--warn); color: var(--warn); }
  @media (max-width: 860px) {
    .board { grid-template-columns: 1fr; grid-template-areas: 'songs' 'jobs' 'tiles'; }
  }
</style>
