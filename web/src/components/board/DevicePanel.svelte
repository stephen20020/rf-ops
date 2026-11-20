<script lang="ts">
  import Modal from '../Modal.svelte';
  import { get, post } from '../../lib/api';
  import { plan, type Tile } from '../../lib/plan.svelte';
  import { store } from '../../lib/store.svelte';
  import type { Id } from '../../lib/types';

  let { tile, onclose }: { tile: Tile; onclose: () => void } = $props();

  type Cand = { device: Id; label: string; tier: { tier: string; song?: Id; by?: Id }; retune: boolean; flag: string | null };
  let picking = $state(false);
  let cands = $state<Cand[]>([]);
  let performer = $state<Id | null>(null);
  let range = $state<'until_fixed' | 'this_song' | 'rest_of_show'>('until_fixed');
  let component = $state('unknown');

  const swapHere = $derived(plan.swaps.find((s) => s.active && (s.failed_id === tile.id || s.replacement_id === tile.id) && s.state !== 'returned'));
  const people = $derived(store.snapshot?.performers ?? []);
  const holderId = $derived(tile.holder.kind === 'person' ? tile.holder.id : null);

  async function startSwap() {
    const r = await get<{ performer: Id | null; candidates: Cand[] }>(`/api/shows/${store.showId}/devices/${tile.id}/candidates`);
    cands = r.candidates;
    performer = r.performer;
    picking = true;
  }
  function tierText(c: Cand) {
    if (c.tier.tier === 'spare') return 'spare';
    if (c.tier.tier === 'free_rest_of_show') return 'free rest of show';
    return `free until ${plan.song(c.tier.song)} (${plan.person(c.tier.by)})`;
  }
  async function doSwap(c: Cand) {
    const ok = await store.act(
      () => post(`/api/shows/${store.showId}/swaps`, { failed: tile.id, replacement: c.device, performer, range, component }),
      `Swap recorded: ${tile.label} → ${c.label}`,
    );
    if (ok) onclose();
  }
  async function pull() {
    const ok = await store.act(() => post(`/api/shows/${store.showId}/swaps`, { kind: 'pull', failed: tile.id, performer, range }), `${tile.label} pulled`);
    if (ok) onclose();
  }
  const setStatus = (status: string) => store.act(() => post(`/api/devices/${tile.id}/status`, { status }), `${tile.label}: ${status}`);
  const setHolder = (p: Id | null) => store.act(() => post(`/api/devices/${tile.id}/holder`, { performer: p }), `${tile.label} is with ${plan.person(p)}`);
  async function battery(r: 'ok' | 'fresh' | 'low') {
    await plan.battery(tile.id, r);
  }

  // Problem presets (interface spec §6.3): 2 taps from a tile. No audio / dropout are critical
  // while the device is in use.
  const inUse = $derived(tile.usage.usage === 'in_use' || tile.usage.usage === 'on_spare_for');
  const who = $derived(tile.holder.kind === 'person' ? plan.person(tile.holder.id) : '');
  async function problem(what: string, critical: boolean) {
    const ok = await store.act(
      () =>
        post('/api/alerts', {
          severity: critical && inUse ? 'critical' : 'warning',
          title: `${tile.label}${who ? ` (${who})` : ''}: ${what}`,
          show_id: store.showId,
          performer_id: tile.holder.kind === 'person' ? tile.holder.id : null,
        }),
      'Reported',
    );
    if (ok) onclose();
  }
</script>

<Modal title={`${tile.label}${tile.mix ? ' · ' + tile.mix : ''}`} {onclose}>
  {#if !picking}
    <section class="grid">
      <div class="muted">Battery</div>
      <div class="row">
        <button class="big" onclick={() => battery('ok')}>✓ OK</button>
        <button class="big" onclick={() => battery('fresh')}>Fresh</button>
        <button class="big warn" onclick={() => battery('low')}>LOW</button>
      </div>

      <div class="muted">Report a problem</div>
      <div class="row">
        <button class="big danger" onclick={() => problem('No audio', true)}>No audio</button>
        <button class="big" onclick={() => problem('Dropouts', true)}>Dropouts</button>
        <button class="big" onclick={() => problem('Crackle / noise', false)}>Crackle</button>
        <button class="big" onclick={() => problem('Low battery', false)}>Low batt</button>
        <button class="big" onclick={() => problem(tile.kind === 'iem' ? 'Wrong mix / pack' : 'Wrong mic', false)}>Wrong {tile.kind === 'iem' ? 'pack' : 'mic'}</button>
      </div>

      <div class="muted">Fix it</div>
      <div class="row">
        <button class="big danger" onclick={startSwap} data-testid="swap-start">Swap…</button>
        <button class="big" onclick={() => setStatus('suspect')}>Suspect</button>
        {#if tile.usage.usage === 'suspect' || tile.usage.usage === 'out' || tile.usage.usage === 'missing'}
          <button class="big" onclick={() => setStatus('ok')}>Fixed / found</button>
        {/if}
        <button class="ghost" onclick={() => confirm(`Mark ${tile.label} out of service?`) && setStatus('out')}>Out of service</button>
      </div>
      {#if swapHere}<div class="muted">This device is in a swap: see the swap card.</div>{/if}

      <div class="muted">Who has it now (fix if wrong)</div>
      <select value={holderId ?? ''} onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; setHolder(v ? Number(v) : null); }}>
        <option value="">In stock</option>
        {#each people as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
    </section>
  {:else}
    <section class="grid">
      <div>Swap <b>{plan.person(performer)}</b> off <b>{tile.label}</b>:</div>
      <div class="row">
        <label>For
          <select bind:value={range}>
            <option value="until_fixed">until {tile.label} is fixed</option>
            <option value="this_song">this song only</option>
            <option value="rest_of_show">rest of show (everyone on {tile.label})</option>
          </select>
        </label>
        <label>What failed
          <select bind:value={component}>
            <option value="unknown">Don't know</option><option value="carrier">Mic / pack</option>
            <option value="element">Capsule / cable</option><option value="channel">Channel / receiver</option>
          </select>
        </label>
      </div>
      {#each cands as c (c.device)}
        <button class="cand" data-testid="swap-option" onclick={() => doSwap(c)}>
          <b>Swap {plan.person(performer)}: {tile.label} → {c.label}</b>
          <span class="muted">{tierText(c)}{c.retune ? ' · sync to the same TX first' : ''}{c.flag ? ` · ${c.flag}` : ''}</span>
        </button>
      {:else}
        <p class="warnbox">No free {tile.kind === 'iem' ? 'IEM pack' : tile.kind === 'guitar' ? 'guitar pack' : 'mic'} of this kind.</p>
      {/each}
      <div class="row">
        <button class="big" onclick={pull}>Pull {tile.label} (no replacement, mute at FOH)</button>
        <button class="ghost" onclick={() => (picking = false)}>Back</button>
      </div>
    </section>
  {/if}
</Modal>

<style>
  .grid { display: grid; gap: 10px; }
  .cand { display: grid; gap: 2px; text-align: left; min-height: 72px; padding: 8px 12px; }
  .warnbox { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--warn); color: var(--warn); margin: 0; }
</style>
