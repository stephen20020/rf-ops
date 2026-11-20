<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { timeAgo } from '../lib/format';
  import type { KitRef } from '../lib/types';

  function where(k: KitRef) {
    if (k.iem_mix) return k.iem_mix;
    return k.console_channel != null ? `Ch ${k.console_channel}` : k.name;
  }
</script>

{#each store.swapBanners as b (b.seq)}
  <div class="banner" role="alert" data-testid="swap-banner">
    <span class="tag">SWAP</span>
    <span class="text">
      <strong>{b.swap.performer_name}</strong> now on <strong class="ch">{where(b.swap.to_kit)}</strong>
      <span class="muted">({b.swap.to_kit.name})</span>
      — was {where(b.swap.from_kit)} <span class="muted">({b.swap.from_kit.name})</span>
      {#if b.swap.scope === 'song'}<span class="pill">this song only</span>{/if}
      <span class="meta">· {b.actor}, {timeAgo(b.at, store.now)}</span>
    </span>
    <button class="big" onclick={() => (store.dismissedSwaps = [...store.dismissedSwaps, b.seq])}>Got it</button>
  </div>
{/each}

<style>
  .banner {
    display: flex; gap: 12px; align-items: center; flex-wrap: wrap;
    margin: 8px 12px 0; padding: 10px 14px; border-radius: var(--radius);
    background: color-mix(in srgb, var(--warn) 22%, var(--panel)); border: 2px solid var(--warn);
    font-size: 1.1rem;
  }
  .tag { background: var(--warn); color: #1b1300; font-weight: 800; padding: 2px 8px; border-radius: 6px; letter-spacing: 0.05em; }
  .text { flex: 1; min-width: 200px; }
  .ch { font-size: 1.3rem; }
  .meta { color: var(--muted); font-size: 0.85rem; }
</style>
