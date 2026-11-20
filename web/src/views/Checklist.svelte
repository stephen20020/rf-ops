<script lang="ts">
  // Pre-show checklist (interface spec §12.1). Auto items tick themselves; the rest are tapped.
  import { del, get, patch, post } from '../lib/api';
  import { plan } from '../lib/plan.svelte';
  import { store } from '../lib/store.svelte';

  type Item = { key: string; label: string; auto: boolean; ok: boolean; detail: string; ticked_by: string | null; ticked_at: string | null; custom_id?: number };
  let data = $state<{ items: Item[]; done: number; total: number; mode: string } | null>(null);
  let newItem = $state('');

  async function load() {
    if (store.showId) data = await get(`/api/shows/${store.showId}/checklist`);
  }
  $effect(() => {
    void plan.data; // reload on every change (batteries, handoffs…)
    void load();
  });
  $effect(() => {
    if (store.showId) void plan.load();
  });

  const tick = (i: Item) => store.act(() => post(`/api/shows/${store.showId}/checklist/tick`, { key: i.key, ticked: !i.ok }));
  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!newItem.trim()) return;
    const ok = await store.act(() => post(`/api/shows/${store.showId}/checklist/custom`, { label: newItem.trim() }));
    if (ok !== undefined) newItem = '';
  }
  const remove = (i: Item) => store.act(() => del(`/api/checklist/custom/${i.custom_id}`));
  async function start() {
    if (!data) return;
    const open = data.items.filter((i) => !i.ok);
    if (open.length && !confirm(`${open.length} item(s) still open:\n\n${open.map((i) => '• ' + i.label).join('\n')}\n\nStart the show anyway?`)) return;
    const ok = await store.act(() => patch(`/api/shows/${store.showId}`, { mode: 'show' }), 'Show mode on');
    if (ok !== undefined) location.hash = '#/board';
  }
  async function end() {
    if (!confirm('End the show? (Show mode off. Checks after this count for the next show.)')) return;
    await store.act(() => patch(`/api/shows/${store.showId}`, { mode: 'rehearsal' }), 'Show ended');
    location.hash = '#/post-show';
  }
</script>

<div class="cl">
  <div class="row head">
    <h1 class="grow">Pre-show — {store.show?.show.name ?? ''}</h1>
    {#if data}<span class="pill" class:ok={data.done === data.total} data-testid="checklist-count">{data.done} / {data.total}</span>{/if}
    <a class="btn ghost" href="#/print" target="_blank" rel="noopener">Print paper plot</a>
  </div>

  {#if data}
    <ul>
      {#each data.items as i (i.key)}
        <li class:done={i.ok} data-testid="check-item" data-key={i.key}>
          {#if i.auto}
            <span class="mark" aria-label={i.ok ? 'done' : 'not yet'}>{i.ok ? '✓' : '○'}</span>
          {:else}
            <button class="mark big" class:primary={i.ok} onclick={() => tick(i)} aria-label={i.ok ? 'Untick' : 'Tick'}>{i.ok ? '✓' : ''}</button>
          {/if}
          <div class="grow">
            <div class="label">{i.label} {#if i.auto}<span class="pill">auto</span>{/if}</div>
            <div class="muted small">
              {i.detail}{#if i.ticked_by} · ✓ {i.ticked_by} {new Date(i.ticked_at ?? '').toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}{/if}
            </div>
          </div>
          {#if i.custom_id}<button class="ghost" onclick={() => remove(i)}>Remove</button>{/if}
        </li>
      {/each}
    </ul>
    <form class="row" onsubmit={add}>
      <input class="grow" placeholder="Add an item for this show" bind:value={newItem} aria-label="New checklist item" />
      <button>Add</button>
    </form>
    <div class="row">
      {#if data.mode === 'show'}
        <span class="pill crit">SHOW MODE</span>
        <button class="big" onclick={end} data-testid="end-show">End show → report</button>
      {:else}
        <button class="primary big" onclick={start} data-testid="start-show">Start show (show mode)</button>
      {/if}
    </div>
  {:else}
    <p class="muted">Loading…</p>
  {/if}
</div>

<style>
  .cl { display: grid; gap: 12px; max-width: 900px; }
  ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  li { display: flex; gap: 12px; align-items: center; padding: 10px; border-radius: 10px; background: var(--panel); border: 1px solid var(--warn); }
  li.done { border-color: var(--line); }
  .mark { width: 48px; min-width: 48px; height: 48px; display: grid; place-items: center; font-size: 1.4rem; font-weight: 800; border-radius: 10px; border: 1px solid var(--line); padding: 0; }
  li.done .mark { color: var(--ok); }
  .label { font-weight: 650; }
  .small { font-size: 0.85rem; }
  .btn { display: inline-flex; align-items: center; text-decoration: none; }
</style>
