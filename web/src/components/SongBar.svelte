<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';

  let { compact = false }: { compact?: boolean } = $props();

  const show = $derived(store.show);
  const active = $derived(store.songs.filter((s) => !s.skipped));
  const current = $derived(show?.show.current_song_id ?? null);
  const index = $derived(current == null ? -1 : active.findIndex((s) => s.id === current));
  const next = $derived(show?.next_song_id ?? null);

  const midi = $derived(store.snapshot?.midi ?? null);
  /** Does this show use MIDI cues at all? */
  const usesMidi = $derived(active.some((s) => !!s.midi_trigger));
  const isTarget = $derived(midi?.target_show_id === store.showId);
  const locked = $derived(show?.show.midi_mode === 'lock');
  const bridgeOnline = $derived(!!midi?.bridge?.online);

  function toggleMidi() {
    if (!show) return;
    return store.act(
      () => post(`/api/shows/${show.show.id}/midi-mode`, { mode: locked ? 'follow' : 'lock' }),
      locked ? 'Following Playback MIDI' : 'MIDI locked out — manual only',
    );
  }
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
    {#if usesMidi}
      <div class="midi" data-testid="midi-status">
        {#if !isTarget}
          <span class="pill" title="Playback MIDI drives the show in show mode (or the latest rehearsal)">MIDI → other show</span>
        {:else if locked}
          <span class="pill warn">🔒 MIDI locked</span>
        {:else if bridgeOnline}
          <span class="pill ok">● Following Playback</span>
        {:else}
          <span class="pill crit">MIDI bridge offline</span>
        {/if}
        {#if isTarget}
          <button class="toggle" onclick={toggleMidi} disabled={!store.connected || !store.canEdit} data-testid="midi-toggle">
            {locked ? 'Follow MIDI' : 'Lock MIDI'}
          </button>
        {/if}
      </div>
    {/if}
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
  .midi { display: grid; gap: 6px; justify-items: end; }
  .toggle { min-height: var(--touch); }
  .compact .toggle { min-height: 40px; }
  @media (max-width: 640px) {
    .songbar { flex-wrap: wrap; }
    .midi { order: 3; width: 100%; grid-auto-flow: column; justify-content: space-between; align-items: center; }
  }
</style>
