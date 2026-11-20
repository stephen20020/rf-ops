<script lang="ts">
  import { store } from '../lib/store.svelte';

  let { route }: { route: string } = $props();
  const shows = $derived(store.snapshot?.shows ?? []);
</script>

<header>
  <nav class="row">
    <strong class="brand">RF Ops</strong>
    <a href="#/backstage" class:active={route === 'backstage'}>Backstage</a>
    <a href="#/foh" class:active={route === 'foh'}>FOH</a>
    <a href="#/admin" class:active={route === 'admin'}>Admin</a>
  </nav>
  <div class="row right">
    <select
      aria-label="Show"
      value={store.showId ?? ''}
      onchange={(e) => store.selectShow(Number((e.currentTarget as HTMLSelectElement).value) || null)}
    >
      {#if shows.length === 0}<option value="">No shows yet</option>{/if}
      {#each shows as s (s.id)}
        <option value={s.id}>{s.name}</option>
      {/each}
    </select>
    {#if store.show}
      <span class="pill" class:warn={store.show.show.mode === 'show'}>{store.show.show.mode}</span>
    {/if}
    <span class="conn pill" class:ok={store.connected} class:crit={!store.connected} data-testid="connection">
      {#if store.connected}● Live{:else}● Offline · {store.staleSeconds}s stale{/if}
    </span>
    <a class="op" href="#/login" title="Change operator">{store.operatorName}{store.user ? ` · ${store.user.role}` : ''}</a>
  </div>
</header>

<style>
  header {
    position: sticky; top: 0; z-index: 10;
    display: flex; justify-content: space-between; gap: 8px; flex-wrap: wrap;
    padding: 8px 12px; background: var(--panel); border-bottom: 1px solid var(--line);
  }
  .brand { margin-right: 6px; }
  nav a { padding: 8px 10px; border-radius: 8px; text-decoration: none; color: var(--muted); font-weight: 600; }
  nav a.active { color: var(--text); background: var(--panel-2); }
  .right { margin-left: auto; }
  select { max-width: 220px; }
  .op { color: var(--muted); text-decoration: none; font-size: 0.9rem; }
</style>
