<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { get, patch, post, put } from '../../lib/api';
  import { timeAgo } from '../../lib/format';
  import type { MidiStatus, MidiTrigger } from '../../lib/types';

  let status = $state<MidiStatus | null>(null);
  let key = $state('');
  let nextNote = $state('');
  let prevNote = $state('');

  // The monitor isn't pushed over the event stream (MIDI can be chatty), so poll while open.
  $effect(() => {
    let alive = true;
    const tick = async () => {
      try {
        status = await get<MidiStatus>('/api/midi/status');
      } catch {
        /* offline indicator covers this */
      }
      if (alive) setTimeout(tick, 1000);
    };
    void tick();
    return () => {
      alive = false;
    };
  });

  const channel = $derived(status?.channel ?? 16);
  const isTarget = $derived(status?.target_show_id === store.showId);
  const serverUrl = $derived(`http://${location.host}`);

  function pcValue(trigger: string | null): string {
    try {
      const t = trigger ? JSON.parse(trigger) : null;
      return t?.type === 'program_change' ? String(t.value) : '';
    } catch {
      return '';
    }
  }

  async function rotate() {
    if (!confirm('Make a new bridge key? Any existing key stops working, and the bridge on the Playback Mac must be restarted with the new one.')) return;
    const r = await store.act(() => post<{ key: string }>('/api/midi/key/rotate'), 'New bridge key made');
    if (r) key = r.key;
  }
  function setChannel(v: string) {
    const c = Number(v);
    if (c >= 1 && c <= 16) return store.act(() => put('/api/midi/settings', { channel: c }), `New cues use channel ${c}`);
  }
  function noteTrigger(v: string): MidiTrigger | null {
    return v === '' ? null : { type: 'note_on', channel, note: Number(v) };
  }
  function saveNextPrev() {
    return store.act(() => put('/api/midi/settings', { next_trigger: noteTrigger(nextNote), prev_trigger: noteTrigger(prevNote) }), 'Next/previous cues saved');
  }
  function setSongPc(songId: number, v: string) {
    return store.act(() =>
      patch(`/api/songs/${songId}`, { midi_trigger: v === '' ? null : { type: 'program_change', channel, value: Number(v) } }),
    );
  }
  /** Number every song 1, 2, 3… in setlist order. */
  async function numberAll() {
    if (!confirm(`Set every song's cue to Program Change 1, 2, 3… on channel ${channel} in setlist order?`)) return;
    await store.act(async () => {
      let i = 1;
      for (const s of store.songs) await patch(`/api/songs/${s.id}`, { midi_trigger: { type: 'program_change', channel, value: i++ } });
    }, 'Cues numbered');
  }
  function simulate(value: number) {
    return store.act(() => post('/api/midi/simulate', { type: 'program_change', channel, value }));
  }
  let simValue = $state('');

  $effect(() => {
    if (status?.next_trigger?.type === 'note_on' && nextNote === '') nextNote = String(status.next_trigger.note);
    if (status?.prev_trigger?.type === 'note_on' && prevNote === '') prevNote = String(status.prev_trigger.note);
  });
</script>

<div class="cols">
  <section class="panel grid">
    <h2>Playback bridge</h2>
    {#if status?.bridge}
      {@const b = status.bridge}
      <div class="row">
        <span class="pill {b.online ? 'ok' : 'crit'}" data-testid="bridge-state">{b.online ? '● Online' : '● Offline'}</span>
        <strong>{b.name}</strong>
        <span class="muted">· last heard {timeAgo(b.last_seen_at, store.now)}</span>
      </div>
      <div class="muted">Listening on: <b>{b.port ?? 'no MIDI port yet'}</b>{#if b.ports.length}{' · '}inputs on that Mac: {b.ports.join(', ')}{/if}</div>
    {:else}
      <p class="muted" data-testid="bridge-state">No bridge has connected since the server started.</p>
    {/if}
    <details>
      <summary>Set up the bridge on the Playback Mac</summary>
      <ol class="steps">
        <li>On the Playback Mac, in the <code>rf-ops</code> folder, build it once: <code>cargo build --release -p rfops-bridge</code></li>
        <li>
          {#if key}
            Run: <code class="copy">./target/release/rfops-bridge --server {serverUrl} --key {key}</code>
            <br /><small>Copy it now: the key is shown only once. RF Ops keeps only a fingerprint of it.</small>
          {:else}
            <button onclick={rotate} disabled={!store.isAdmin}>Make a bridge key</button> (Lead only; replaces any existing key)
          {/if}
        </li>
        <li>In Playback, set <b>MIDI Out</b> to <b>RF Ops</b> (the bridge creates it). MIDI Out requires the MultiTracks Cloud subscription.</li>
        <li>Give each song a Program Change on channel {channel} matching the numbers below.</li>
        <li>To start it automatically, see <i>docs/operations.md → MIDI bridge</i>.</li>
      </ol>
    </details>
    {#if !isTarget && status}
      <p class="warnbox">MIDI currently drives a different show (the one in show mode, or the latest rehearsal). Put this show in rehearsal or show mode to follow it.</p>
    {/if}
  </section>

  <section class="panel grid">
    <h2>Cues</h2>
    <label>Channel for song cues
      <select value={String(channel)} onchange={(e) => setChannel((e.currentTarget as HTMLSelectElement).value)} disabled={!store.isAdmin}>
        {#each Array.from({ length: 16 }, (_, i) => i + 1) as c (c)}<option value={String(c)}>{c}</option>{/each}
      </select>
    </label>
    <div class="row">
      <label>"Next song" note (optional)<input type="number" min="0" max="127" bind:value={nextNote} placeholder="e.g. 60" /></label>
      <label>"Previous song" note<input type="number" min="0" max="127" bind:value={prevNote} placeholder="e.g. 59" /></label>
      <button onclick={saveNextPrev} disabled={!store.isAdmin}>Save</button>
    </div>
    <p class="muted small">Each song listens for its own Program Change, so a skipped or reordered song still lands correctly. Next/previous notes are optional extras.</p>
  </section>

  {#if store.show}
    <section class="panel grid">
      <div class="row">
        <h2 class="grow">Song cues · {store.show.show.name}</h2>
        <button class="ghost" onclick={numberAll} disabled={store.showMode}>Number 1, 2, 3…</button>
      </div>
      <table>
        <thead><tr><th>#</th><th>Song</th><th>Program Change (ch {channel})</th><th>Test</th></tr></thead>
        <tbody>
          {#each store.songs as s (s.id)}
            {@const pc = pcValue(s.midi_trigger)}
            <tr class:skipped={s.skipped} class:current={s.id === store.show.show.current_song_id}>
              <td>{s.position}</td>
              <td>{s.title}{#if s.skipped} <span class="pill">skipped</span>{/if}</td>
              <td><input type="number" min="0" max="127" value={pc} aria-label="Program Change for {s.title}" onchange={(e) => setSongPc(s.id, (e.currentTarget as HTMLInputElement).value)} /></td>
              <td>{#if pc !== ''}<button onclick={() => simulate(Number(pc))} data-testid="simulate-song">Send PC {pc}</button>{/if}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="row">
        <input type="number" min="0" max="127" bind:value={simValue} placeholder="PC value" aria-label="Simulate Program Change value" />
        <button onclick={() => simValue !== '' && simulate(Number(simValue))}>Simulate Program Change</button>
      </div>
    </section>
  {/if}

  <section class="panel grid">
    <h2>MIDI monitor</h2>
    <table class="monitor" data-testid="midi-monitor">
      <thead><tr><th>Time</th><th>From</th><th>Message</th><th>Result</th></tr></thead>
      <tbody>
        {#each status?.recent ?? [] as m, i (i)}
          <tr>
            <td>{new Date(m.at).toLocaleTimeString()}</td>
            <td>{m.source}</td>
            <td>{m.message}</td>
            <td class:bad={/LOCKED|warning|NOT/.test(m.outcome)} class:good={m.outcome.startsWith('→')}>{m.outcome}</td>
          </tr>
        {:else}
          <tr><td colspan="4" class="muted">Nothing received yet.</td></tr>
        {/each}
      </tbody>
    </table>
  </section>
</div>

<style>
  .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; align-items: start; }
  .grid { display: grid; gap: 10px; }
  .steps { display: grid; gap: 8px; padding-left: 20px; }
  code { background: var(--bg); padding: 2px 6px; border-radius: 6px; font-size: 0.85rem; }
  .copy { display: block; margin-top: 4px; white-space: pre-wrap; word-break: break-all; user-select: all; }
  .small { font-size: 0.85rem; margin: 0; }
  .warnbox { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--warn); color: var(--warn); margin: 0; }
  td input { width: 90px; }
  tr.skipped td { color: var(--muted); }
  tr.current td { color: var(--accent); font-weight: 700; }
  .monitor td { font-size: 0.85rem; }
  .bad { color: var(--warn); }
  .good { color: var(--ok); }
  @media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
</style>
