<script lang="ts">
  import { post } from '../../lib/api';
  import { store } from '../../lib/store.svelte';
  import type { Swap } from '../../lib/plan.svelte';

  let { s, foh = false }: { s: Swap; foh?: boolean } = $props();

  const step = (path: string, msg: string) => store.act(() => post(`/api/swaps/${s.id}/${path}`), msg);
  const title = $derived(
    s.kind === 'pull'
      ? `Pull ${s.failed} (${s.performer ?? '—'})`
      : s.kind === 'tx_swap'
        ? `TX swap ${s.failed_base} → ${s.replacement_base}`
        : `${s.performer ?? '—'}: ${s.failed} → ${s.replacement}`,
  );
  const console_line = $derived(
    s.kind === 'pull'
      ? `Mute Ch ${s.failed_channel ?? '?'} — no replacement`
      : s.failed_channel || s.replacement_channel
        ? `Ch ${s.failed_channel ?? '—'} → Ch ${s.replacement_channel ?? '—'}${s.failed_channel && s.replacement_channel ? `. If Ch ${s.replacement_channel} isn't set like Ch ${s.failed_channel}, copy processing.` : ''}`
        : 'Pack only — no console change',
  );
</script>

<div class="swap" data-testid="swap-card" data-state={s.state}>
  <div class="grow">
    <div class="what">{title}</div>
    <div class="muted small">{console_line}</div>
    {#if s.retune && s.state === 'recorded'}<div class="small warnline">Sync {s.replacement} to {s.failed}'s TX before handing it over (keeps the mix{s.mix ? `: ${s.mix}` : ''}).</div>{/if}
    <div class="muted small">
      {#if s.state === 'recorded'}Getting ready · {s.created_by}
      {:else if s.state === 'in_hand'}In hand · waiting for FOH
      {:else if s.state === 'foh_confirmed'}FOH ✓ · until {s.range === 'this_song' ? 'end of song' : s.range === 'rest_of_show' ? 'end of show' : 'fixed'}
      {:else if s.state === 'return_recorded'}Returning to {s.failed}
      {:else if s.state === 'planned'}Planned for a later song{/if}
    </div>
  </div>
  <div class="actions">
    {#if foh}
      {#if s.state === 'in_hand'}<button class="primary big" onclick={() => step('foh-done', 'Confirmed at console')}>Done at console</button>{/if}
    {:else if s.state === 'recorded'}
      <button class="primary big" onclick={() => step('in-hand', 'In hand')}>In hand</button>
      <button class="ghost" onclick={() => step('reverse', 'Swap cancelled')}>Undo</button>
    {:else if s.state === 'in_hand' || s.state === 'foh_confirmed'}
      {#if s.kind === 'swap'}<button onclick={() => step('return', 'Returning')}>{s.failed} fixed: return</button>{/if}
      <button class="ghost" onclick={() => step('reverse', 'Swap reversed')}>Reverse</button>
    {:else if s.state === 'return_recorded'}
      <button class="primary big" onclick={() => step('return-in-hand', `Back on ${s.failed}`)}>Back in hand</button>
    {/if}
  </div>
</div>

<style>
  .swap { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 10px; border: 2px solid var(--info); background: var(--panel-2); flex-wrap: wrap; }
  .what { font-weight: 700; }
  .small { font-size: 0.82rem; }
  .warnline { color: var(--warn); }
  .actions { display: flex; gap: 6px; flex-wrap: wrap; }
</style>
