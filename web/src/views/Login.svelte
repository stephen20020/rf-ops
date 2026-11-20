<script lang="ts">
  import { store } from '../lib/store.svelte';

  // Sign-in (docs/interface-spec.md §1.1): name + PIN for Leads, A2s and FOH; a 6-digit code
  // from the Lead for helpers. A QR link opens #/join?code=123456 and signs in directly.
  type Mode = 'pin' | 'code';
  const qCode = new URLSearchParams(location.hash.split('?')[1] ?? '').get('code') ?? '';
  let mode = $state<Mode>(qCode || location.hash.startsWith('#/join') ? 'code' : 'pin');
  let name = $state(loadName());
  let digits = $state(qCode.replace(/\D/g, '').slice(0, 6));
  let busy = $state(false);

  function loadName(): string {
    try {
      return localStorage.getItem('rfops.lastName') ?? '';
    } catch {
      return '';
    }
  }

  const max = $derived(mode === 'pin' ? 8 : 6);
  const ready = $derived(mode === 'pin' ? name.trim().length > 0 && digits.length >= 4 : digits.length === 6);

  async function submit() {
    if (!ready || busy) return;
    busy = true;
    let ok = false;
    if (mode === 'pin') {
      ok = await store.login(name.trim(), digits);
      if (ok) {
        try {
          localStorage.setItem('rfops.lastName', name.trim());
        } catch {
          /* ignore */
        }
      }
    } else {
      ok = await store.join(digits);
    }
    busy = false;
    digits = '';
    if (ok && (location.hash.startsWith('#/login') || location.hash.startsWith('#/join'))) location.hash = '#/backstage';
  }

  function press(k: string) {
    if (k === '⌫') digits = digits.slice(0, -1);
    else if (k === 'OK') void submit();
    else if (digits.length < max) digits += k;
  }

  function setMode(m: Mode) {
    mode = m;
    digits = '';
  }

  // Auto-submit a QR code once.
  $effect(() => {
    if (qCode && digits.length === 6) void submit();
  });
</script>

<svelte:window onkeydown={(e) => {
  if ((e.target as HTMLElement)?.tagName === 'INPUT') {
    if (e.key === 'Enter') void submit();
    return;
  }
  if (/^[0-9]$/.test(e.key)) press(e.key);
  else if (e.key === 'Backspace') press('⌫');
  else if (e.key === 'Enter') press('OK');
}} />

<section class="login">
  <h1>RF Ops</h1>
  {#if !store.adminExists}
    <p class="warnbox" data-testid="not-set-up">
      No Lead can sign in yet. On the server, set <code>RFOPS_ADMIN_NAME</code> and <code>RFOPS_ADMIN_PIN</code>
      and restart, or run <code>rfops-server --create-admin "Your name" 123456</code>.
    </p>
  {/if}
  {#if store.sessionEnded}<p class="warnbox">You were signed out. Please sign in again.</p>{/if}

  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={mode === 'pin'} class:on={mode === 'pin'} onclick={() => setMode('pin')}>Name + PIN</button>
    <button role="tab" aria-selected={mode === 'code'} class:on={mode === 'code'} onclick={() => setMode('code')}>Have a code?</button>
  </div>

  {#if mode === 'pin'}
    <label class="field">
      <span class="muted">Your name</span>
      <input bind:value={name} autocomplete="username" autocapitalize="words" data-testid="login-name" />
    </label>
    <p class="muted">PIN</p>
  {:else}
    <p class="muted">Type the 6-digit code the Lead gave you. On iPhone, if you added RF Ops to your Home Screen, type the code again there.</p>
  {/if}

  <div class="dots" aria-label="{mode === 'pin' ? 'PIN' : 'code'} entered" data-testid="pin-dots">
    {#each Array(Math.max(mode === 'pin' ? 4 : 6, digits.length)) as _, i (i)}<span class:on={i < digits.length}></span>{/each}
  </div>
  <div class="pad">
    {#each ['1', '2', '3', '4', '5', '6', '7', '8', '9', '⌫', '0', 'OK'] as k (k)}
      <button class="key" class:primary={k === 'OK'} disabled={busy || (k === 'OK' && !ready)} onclick={() => press(k)}>{k}</button>
    {/each}
  </div>
</section>

<style>
  .login { max-width: 480px; margin: 5vh auto; padding: 16px; display: grid; gap: 14px; }
  .tabs { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .tabs button { min-height: 48px; }
  .tabs button.on { border-color: var(--accent); font-weight: 600; }
  .field { display: grid; gap: 4px; }
  .field input { min-height: 48px; font-size: 1.2rem; }
  .pad { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; max-width: 360px; width: 100%; justify-self: center; }
  .key { min-height: 72px; font-size: 1.6rem; }
  .dots { display: flex; gap: 12px; justify-content: center; }
  .dots span { width: 18px; height: 18px; border-radius: 50%; border: 2px solid var(--muted); }
  .dots span.on { background: var(--text); border-color: var(--text); }
  .warnbox { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--warn); color: var(--warn); margin: 0; }
  code { font-size: 0.9em; }
</style>
