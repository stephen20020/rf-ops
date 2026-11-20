<script lang="ts">
  import { store } from '../lib/store.svelte';

  let { compact = false }: { compact?: boolean } = $props();

  const show = $derived(store.show);
  const active = $derived(store.songs.filter((s) => !s.skipped));
  const current = $derived(show?.show.current_song_id ?? null);
  const index = $derived(current == null ? -1 : active.findIndex((s) => s.id === current));
  const next = $derived(show?.next_song_id ?? null);
</script>

{#if show}
  <div class="songbar panel" class:compact>
    <button class="big nav" onclick={() => store.advance('prev')} disabled={!store.connected || index <= 0} aria-label="Previous song">◀</button>
    <div class="grow titles">
      <div class="muted small">
        {#if index >= 0}Now · {index + 1} of {active.length}{:else}Before show{/if}
      </div>
      <div class="now" data-testid="current-song">{current == null ? 'Pre-show' : store.songTitle(current)}</div>
      <div class="muted next">Next: <span data-testid="next-song">{next == null ? (current == null ? '—' : 'End of show') : store.songTitle(next)}</span></div>
    </div>
    <button class="big nav primary" onclick={() => store.advance('next')} disabled={!store.connected || next == null} aria-label="Next song">▶</button>
  </div>
{/if}

<style>
  .songbar { display: flex; align-items: center; gap: 12px; }
  .titles { min-width: 0; }
  .now { font-size: 1.6rem; font-weight: 750; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .compact .now { font-size: 1.2rem; }
  .next { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .small { font-size: 0.8rem; }
  .nav { min-width: 64px; min-height: 64px; font-size: 1.4rem; }
  .compact .nav { min-width: 48px; min-height: 48px; }
</style>
