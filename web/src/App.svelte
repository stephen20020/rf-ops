<script lang="ts">
  import { store } from './lib/store.svelte';
  import TopBar from './components/TopBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import Login from './views/Login.svelte';
  import Backstage from './views/Backstage.svelte';
  import Foh from './views/Foh.svelte';
  import Admin from './views/Admin.svelte';
  import Report from './views/Report.svelte';
  import SwapBanner from './components/SwapBanner.svelte';

  type Route = 'backstage' | 'foh' | 'admin' | 'login' | 'report';
  function parse(): Route {
    const h = location.hash.replace(/^#\/?/, '').split('?')[0];
    return (['backstage', 'foh', 'admin', 'login', 'report'] as Route[]).includes(h as Route) ? (h as Route) : 'backstage';
  }
  let route = $state<Route>(parse());
  $effect(() => {
    const on = () => (route = parse());
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  });

  // Ask for an operator first (unless just viewing the login screen).
  const needsLogin = $derived(store.loaded && (!store.operatorName || store.needsPin));
</script>

{#if !store.loaded}
  <div class="loading muted">Connecting to RF Ops server…</div>
{:else if needsLogin || route === 'login'}
  <Login />
{:else if route === 'report'}
  <Report />
{:else}
  <TopBar {route} />
  <SwapBanner />
  <main>
    {#if route === 'foh'}
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
