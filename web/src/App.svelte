<script lang="ts">
  import { store } from './lib/store.svelte';
  import TopBar from './components/TopBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import Login from './views/Login.svelte';
  import Backstage from './views/Backstage.svelte';
  import Foh from './views/Foh.svelte';
  import Admin from './views/Admin.svelte';
  import Report from './views/Report.svelte';
  import Takeover from './views/Takeover.svelte';
  import Board from './views/Board.svelte';
  import Channels from './views/Channels.svelte';
  import SwapBanner from './components/SwapBanner.svelte';

  type Route = 'board' | 'channels' | 'backstage' | 'foh' | 'admin' | 'login' | 'join' | 'report' | 'takeover';
  const ROUTES: Route[] = ['board', 'channels', 'backstage', 'foh', 'admin', 'login', 'join', 'report', 'takeover'];
  function parse(): Route {
    const h = location.hash.replace(/^#\/?/, '').split('?')[0];
    return ROUTES.includes(h as Route) ? (h as Route) : 'board';
  }
  let route = $state<Route>(parse());
  $effect(() => {
    const on = () => (route = parse());
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  });

</script>

{#if route === 'takeover'}
  <Takeover />
{:else if store.fenced}
  <div class="loading" data-testid="fenced">
    <p><b>This is not the active RF Ops server.</b></p>
    <p class="muted">Open the main RF Ops address. Lead: to make this server active, go to <a href="#/takeover">Take over</a>.</p>
  </div>
{:else if !store.authChecked}
  <div class="loading muted">Connecting to RF Ops server…</div>
{:else if !store.me || route === 'login' || route === 'join'}
  <Login />
{:else if !store.loaded}
  <div class="loading muted">Loading…</div>
{:else if route === 'report'}
  <Report />
{:else}
  <TopBar {route} />
  <SwapBanner />
  <main>
    {#if route === 'board'}
      <Board />
    {:else if route === 'channels'}
      <Channels />
    {:else if route === 'foh'}
      <Foh />
    {:else if route === 'admin'}
      <Admin />
    {:else}
      <Backstage />
    {/if}
  </main>
{/if}
<Toasts />

<style>
  .loading { padding: 40px; text-align: center; }
  main { padding: 12px; }
  @media (max-width: 640px) {
    main { padding: 8px; }
  }
</style>
