<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { post } from '../lib/api';
  import { doneKey, doneMap, handoffItem, handoffPerformers } from '../lib/format';
  import type { Handoff, HandoffDone, Id, Transition } from '../lib/types';

  let { transitions, onlyPerformers = null }: { transitions: Transition[]; onlyPerformers?: Set<Id> | null } = $props();

  const done = $derived(doneMap(store.show?.handoffs_done ?? []));

  /** One tappable line. Issues/returns are grouped per performer so pre-show isn't 20 rows. */
  interface Row {
    key: string;
    kind: Handoff['type'];
    performer: Id;
    from?: Id;
    items: Handoff[];
  }

  function rowsFor(t: Transition): Row[] {
    const rows: Row[] = [];
    const grouped = new Map<string, Row>();
    for (const h of t.handoffs) {
      if (onlyPerformers && !handoffPerformers(h).some((p) => onlyPerformers!.has(p))) continue;
      if (h.type === 'issue' || h.type === 'return') {
        const performer = h.type === 'issue' ? h.to_performer : h.from_performer;
        const key = `${h.type}-${performer}`;
        let r = grouped.get(key);
        if (!r) {
          r = { key, kind: h.type, performer, items: [] };
          grouped.set(key, r);
          rows.push(r);
        }
        r.items.push(h);
      } else if (h.type === 'transfer') {
        rows.push({ key: `t-${h.item_id}`, kind: h.type, performer: h.to_performer, from: h.from_performer, items: [h] });
      } else {
        rows.push({ key: `c-${h.carrier_id}`, kind: h.type, performer: h.performer_id, items: [h] });
      }
    }
    // Transfers and swaps first: they need two people / a tech at the same moment.
    const order = { transfer: 0, component_change: 1, issue: 2, return: 3 };
    return rows.sort((a, b) => order[a.kind] - order[b.kind]);
  }

  function doneOf(t: Transition, r: Row): (HandoffDone | undefined)[] {
    return r.items.map((h) => done.get(doneKey(t.from_song, t.to_song, handoffItem(h), h.type)));
  }

  async function toggle(t: Transition, r: Row) {
    const states = doneOf(t, r);
    const allDone = states.every(Boolean);
    const targets = r.items.filter((_, i) => (allDone ? true : !states[i]));
    await store.act(async () => {
      for (const h of targets) {
        await post(`/api/shows/${store.showId}/handoffs/done`, {
          from_song: t.from_song,
          to_song: t.to_song,
          item_id: handoffItem(h),
          type: h.type,
          done: !allDone,
        });
      }
    });
  }

  const labels = (items: Handoff[]) => items.map((h) => store.label(handoffItem(h))).join(', ');
</script>

<div class="handoffs" data-testid="handoffs">
  {#each transitions as t (`${t.from_song}-${t.to_song}`)}
    {@const rows = rowsFor(t)}
    <section>
      <h3>{t.to_song == null ? 'After show' : `Before ${store.songTitle(t.to_song)}`} <span class="muted">· {rows.length}</span></h3>
      <ul>
        {#each rows as r (r.key)}
          {@const states = doneOf(t, r)}
          {@const all = states.every(Boolean)}
          {@const some = !all && states.some(Boolean)}
          {@const h = r.items[0]}
          <li>
            <button class="item {r.kind}" class:done={all} onclick={() => toggle(t, r)} disabled={!store.connected || !store.canEdit} data-testid="handoff" data-kind={r.kind}>
              <span class="check" aria-hidden="true">{all ? '✔' : some ? '–' : ''}</span>
              <span class="text">
                {#if h.type === 'transfer'}
                  <span class="tag swap">HAND OFF</span>
                  <strong>{store.label(h.item_id)}</strong>: {store.performerName(h.from_performer)} → <strong>{store.performerName(h.to_performer)}</strong>
                {:else if h.type === 'component_change'}
                  <span class="tag swap">SWAP</span>
                  <strong>{store.label(h.carrier_id)}</strong>: {store.label(h.from_element)} → <strong>{store.label(h.to_element)}</strong> · {store.performerName(h.performer_id)}
                {:else if r.kind === 'issue'}
                  <span class="tag">GIVE</span>
                  <strong>{store.performerName(r.performer)}</strong> ← {labels(r.items)}
                {:else}
                  <span class="tag">COLLECT</span>
                  <strong>{store.performerName(r.performer)}</strong> → {labels(r.items)}
                {/if}
                {#if all}<span class="muted by">✓ {states[0]?.done_by}</span>{/if}
              </span>
            </button>
          </li>
        {:else}
          <li class="muted">Nothing for you here.</li>
        {/each}
      </ul>
    </section>
  {:else}
    <p class="muted">No upcoming handoffs.</p>
  {/each}
</div>

<style>
  .handoffs { display: grid; gap: 12px; }
  ul { list-style: none; margin: 6px 0 0; padding: 0; display: grid; gap: 6px; }
  .item { width: 100%; min-height: var(--touch); display: flex; gap: 10px; align-items: center; text-align: left; font-weight: 400; padding: 8px 12px; }
  .item.transfer, .item.component_change { border-color: color-mix(in srgb, var(--warn) 55%, var(--line)); }
  .item.done { opacity: 0.55; }
  .check { width: 26px; height: 26px; flex: none; border-radius: 6px; border: 2px solid var(--line); display: grid; place-items: center; color: var(--ok); font-weight: 800; }
  .text { min-width: 0; }
  .tag { font-size: 0.7rem; font-weight: 800; letter-spacing: 0.05em; color: var(--muted); margin-right: 4px; }
  .tag.swap { color: var(--warn); }
  .by { margin-left: 6px; font-size: 0.8rem; }
</style>
