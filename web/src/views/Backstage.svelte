<script lang="ts">
  import { store } from '../lib/store.svelte';
  import SongBar from '../components/SongBar.svelte';
  import AlertList from '../components/AlertList.svelte';
  import HandoffPanel from '../components/HandoffPanel.svelte';
  import PerformerCard from '../components/PerformerCard.svelte';
  import PerformerSheet from '../components/PerformerSheet.svelte';
  import RaiseAlert from '../components/RaiseAlert.svelte';
  import type { Id, Performer } from '../lib/types';

  function loadFilter(): 'all' | 'mine' {
    try {
      return localStorage.getItem('rfops.filter') === 'mine' ? 'mine' : 'all';
    } catch {
      return 'all';
    }
  }
  let filter = $state<'all' | 'mine'>(loadFilter());
  $effect(() => {
    try {
      localStorage.setItem('rfops.filter', filter);
    } catch {
      /* ignore */
    }
  });

  let open = $state<Performer | null>(null);
  let raising = $state(false);

  const show = $derived(store.show);
  const myId = $derived(store.user?.id ?? null);
  const mine = $derived(
    new Set<Id>(
      [...store.performers.values()].filter((p) => myId != null && p.owner_user_id === myId).map((p) => p.id),
    ),
  );
  const scope = $derived(filter === 'mine' && myId != null ? mine : null);

  /** Performers with a kit now or in the next song; on-stage first. */
  const rows = $derived.by(() => {
    if (!show) return [];
    const out: { p: Performer; now: ReturnType<typeof store.kitsFor>; next: ReturnType<typeof store.kitsFor> | null }[] = [];
    for (const p of store.performers.values()) {
      if (scope && !scope.has(p.id)) continue;
      const now = store.kitsFor(show.effective, p.id);
      const next = show.next_effective ? store.kitsFor(show.next_effective, p.id) : null;
      if (now.length === 0 && !next?.length) continue;
      out.push({ p, now, next });
    }
    return out.sort((a, b) => Number(b.now.length > 0) - Number(a.now.length > 0) || a.p.name.localeCompare(b.p.name));
  });

  const alerts = $derived(
    scope
      ? store.alerts.filter((a) => a.performer_id == null || scope.has(a.performer_id))
      : store.alerts,
  );
</script>

{#if !show}
  <div class="panel empty">
    <h2>No show selected</h2>
    <p class="muted">Create a show in <a href="#/admin">Admin</a>, or start the server with <code>--seed-demo</code> to try it with demo data.</p>
  </div>
{:else}
  <SongBar />
  <div class="layout">
    <div class="main">
      <div class="row filters">
        <button class="big" class:primary={filter === 'all'} onclick={() => (filter = 'all')}>All performers</button>
        <button class="big" class:primary={filter === 'mine'} onclick={() => (filter = 'mine')} disabled={myId == null}>Mine{myId != null ? ` (${mine.size})` : ''}</button>
        <span class="grow"></span>
        <button class="danger big" onclick={() => (raising = true)} disabled={!store.connected || !store.canEdit} data-testid="raise-alert">⚠ Raise alert</button>
      </div>
      <div class="cards">
        {#each rows as r (r.p.id)}
          <PerformerCard performer={r.p} now={r.now} next={r.next} onopen={() => (open = r.p)} />
        {:else}
          <p class="muted">No performers with kits {filter === 'mine' ? 'assigned to you ' : ''}in this or the next song.</p>
        {/each}
      </div>
    </div>
    <aside class="side">
      <section class="panel">
        <h2>Alerts <span class="muted">· {alerts.length}</span></h2>
        <AlertList {alerts} />
      </section>
      <section class="panel">
        <h2>Upcoming handoffs</h2>
        <HandoffPanel transitions={show.upcoming} onlyPerformers={scope} />
      </section>
    </aside>
  </div>
{/if}

{#if open}<PerformerSheet performer={open} onclose={() => (open = null)} />{/if}
{#if raising}<RaiseAlert onclose={() => (raising = false)} />{/if}

<style>
  .layout { margin-top: 12px; display: grid; grid-template-columns: minmax(0, 1fr) minmax(320px, 420px); gap: 12px; align-items: start; }
  .main { display: grid; gap: 12px; min-width: 0; }
  .side { display: grid; gap: 12px; position: sticky; top: 64px; max-height: calc(100vh - 76px); overflow: auto; }
  .side h2 { margin-bottom: 8px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; align-items: start; }
  .empty { max-width: 560px; margin: 10vh auto; display: grid; gap: 8px; }
  @media (max-width: 900px) {
    .layout { grid-template-columns: 1fr; }
    .side { position: static; max-height: none; order: -1; }
  }
</style>
