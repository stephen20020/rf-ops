<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { send } from '../lib/api';

  let firstName = $state('');
  const users = $derived(store.snapshot?.users ?? []);

  function choose(name: string) {
    store.setOperator(name);
    location.hash = '#/backstage';
  }

  async function createFirstAdmin(e: SubmitEvent) {
    e.preventDefault();
    const name = firstName.trim();
    if (!name) return;
    store.setOperator(name);
    try {
      await send('POST', '/api/users', { name, role: 'admin' });
      await store.refresh();
      choose(name);
    } catch (err) {
      store.toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }
</script>

<section class="login">
  <h1>RF Ops</h1>
  {#if store.bootstrap}
    <p class="muted">No operators are set up yet. Create the first admin to get started.</p>
    <form class="row" onsubmit={createFirstAdmin}>
      <input class="grow" placeholder="Your name" bind:value={firstName} aria-label="Your name" />
      <button class="primary big" type="submit" disabled={!firstName.trim()}>Create admin</button>
    </form>
  {:else}
    <p class="muted">Who is operating this screen?</p>
    <div class="grid">
      {#each users as u (u.id)}
        <button class="big user" class:current={u.name === store.operatorName} onclick={() => choose(u.name)}>
          <span>{u.name}</span>
          <span class="pill">{u.role}</span>
        </button>
      {/each}
    </div>
    <p class="muted small">PIN sign-in comes in a later phase. Viewers can watch but not change anything.</p>
  {/if}
</section>

<style>
  .login { max-width: 640px; margin: 8vh auto; padding: 16px; display: grid; gap: 16px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
  .user { display: flex; justify-content: space-between; align-items: center; min-height: 64px; }
  .user.current { border-color: var(--accent); }
  .small { font-size: 0.85rem; }
</style>
