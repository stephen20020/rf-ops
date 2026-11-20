<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { send } from '../lib/api';
  import type { User } from '../lib/types';

  let firstName = $state('');
  let picked = $state<User | null>(null);
  let pin = $state('');
  let busy = $state(false);
  const users = $derived(store.snapshot?.users ?? []);

  async function signIn(u: User, p: string) {
    busy = true;
    const ok = await store.login(u.name, p);
    busy = false;
    if (ok) {
      picked = null;
      pin = '';
      location.hash = '#/backstage';
    } else {
      pin = '';
    }
  }

  function choose(u: User) {
    if (u.has_pin) {
      picked = u;
      pin = '';
    } else {
      void signIn(u, '');
    }
  }

  function press(k: string) {
    if (k === '⌫') pin = pin.slice(0, -1);
    else if (k === 'OK') {
      if (picked && pin.length >= 4) void signIn(picked, pin);
    } else if (pin.length < 8) pin += k;
  }

  async function createFirstAdmin(e: SubmitEvent) {
    e.preventDefault();
    const name = firstName.trim();
    if (!name) return;
    store.setOperator(name);
    try {
      await send('POST', '/api/users', { name, role: 'admin' });
      await store.refresh();
      location.hash = '#/backstage';
    } catch (err) {
      store.toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }
</script>

<svelte:window onkeydown={(e) => {
  if (!picked) return;
  if (/^[0-9]$/.test(e.key)) press(e.key);
  else if (e.key === 'Backspace') press('⌫');
  else if (e.key === 'Enter') press('OK');
  else if (e.key === 'Escape') picked = null;
}} />

<section class="login">
  <h1>RF Ops</h1>
  {#if store.bootstrap}
    <p class="muted">No operators are set up yet. Create the first admin to get started.</p>
    <form class="row" onsubmit={createFirstAdmin}>
      <input class="grow" placeholder="Your name" bind:value={firstName} aria-label="Your name" />
      <button class="primary big" type="submit" disabled={!firstName.trim()}>Create admin</button>
    </form>
  {:else if picked}
    <p class="muted">PIN for <strong>{picked.name}</strong></p>
    <div class="dots" aria-label="PIN entered" data-testid="pin-dots">
      {#each Array(Math.max(4, pin.length)) as _, i (i)}<span class:on={i < pin.length}></span>{/each}
    </div>
    <div class="pad">
      {#each ['1', '2', '3', '4', '5', '6', '7', '8', '9', '⌫', '0', 'OK'] as k (k)}
        <button class="key" class:primary={k === 'OK'} disabled={busy || (k === 'OK' && pin.length < 4)} onclick={() => press(k)}>{k}</button>
      {/each}
    </div>
    <button class="ghost" onclick={() => (picked = null)}>Back</button>
  {:else}
    {#if store.needsPin}<p class="warnbox">Your session ended or this operator needs a PIN. Please sign in again.</p>{/if}
    <p class="muted">Who is operating this screen?</p>
    <div class="grid">
      {#each users as u (u.id)}
        <button class="big user" class:current={u.name === store.operatorName} onclick={() => choose(u)}>
          <span>{u.name}</span>
          <span class="row"><span class="pill">{u.role}</span>{#if u.has_pin}<span aria-label="PIN required">🔒</span>{/if}</span>
        </button>
      {/each}
    </div>
    <p class="muted small">Viewers can watch but not change anything. Set PINs in Admin → People.</p>
  {/if}
</section>

<style>
  .login { max-width: 640px; margin: 6vh auto; padding: 16px; display: grid; gap: 16px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 10px; }
  .user { display: flex; justify-content: space-between; align-items: center; min-height: 64px; }
  .user.current { border-color: var(--accent); }
  .small { font-size: 0.85rem; }
  .pad { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; max-width: 360px; width: 100%; justify-self: center; }
  .key { min-height: 72px; font-size: 1.6rem; }
  .dots { display: flex; gap: 12px; justify-content: center; }
  .dots span { width: 18px; height: 18px; border-radius: 50%; border: 2px solid var(--muted); }
  .dots span.on { background: var(--text); border-color: var(--text); }
  .warnbox { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--warn); color: var(--warn); margin: 0; }
</style>
