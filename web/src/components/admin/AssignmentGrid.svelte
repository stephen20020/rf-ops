<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { put } from '../../lib/api';
  import type { Assignment, Conflict, Id, KitKind } from '../../lib/types';

  let kind = $state<KitKind>('mic');
  let onlyAssigned = $state(true);

  const show = $derived(store.show);
  const songs = $derived(store.songs.filter((s) => !s.skipped));
  const kitsOfKind = $derived((show?.kits ?? []).filter((k) => k.kind === kind).sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true })));

  /** (performer, song|null) -> rows of this kind */
  const cells = $derived.by(() => {
    const m = new Map<string, Assignment[]>();
    for (const a of show?.assignments ?? []) {
      if (a.kind !== kind) continue;
      const key = `${a.performer_id}:${a.song_id ?? 'd'}`;
      m.set(key, [...(m.get(key) ?? []), a]);
    }
    return m;
  });

  const performers = $derived(
    [...store.performers.values()]
      .filter((p) => !onlyAssigned || (show?.assignments ?? []).some((a) => a.performer_id === p.id && a.kind === kind))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  /** song|null -> performer ids involved in a conflict */
  const conflicted = $derived.by(() => {
    const m = new Map<string, Set<Id>>();
    const add = (song: Id | null, c: Conflict) => {
      const key = String(song ?? 'd');
      const s = m.get(key) ?? new Set<Id>();
      if (c.type === 'item_shared') c.performer_ids.forEach((p) => s.add(p));
      else if ('performer_id' in c) s.add(c.performer_id);
      m.set(key, s);
    };
    for (const g of show?.conflicts.by_song ?? []) for (const c of g.conflicts) add(g.song_id, c);
    return m;
  });

  function value(performer: Id, song: Id | null): string {
    const rows = cells.get(`${performer}:${song ?? 'd'}`);
    if (!rows) return song == null ? 'none' : 'inherit';
    if (rows.some((r) => r.kit_id == null)) return 'none';
    return rows.length === 1 ? String(rows[0].kit_id) : 'multi';
  }
  function inheritedLabel(performer: Id): string {
    const rows = cells.get(`${performer}:d`) ?? [];
    return rows.map((r) => store.kits.get(r.kit_id ?? -1)?.name ?? '').filter(Boolean).join(', ') || 'none';
  }
  function multiLabel(performer: Id, song: Id | null): string {
    return (cells.get(`${performer}:${song ?? 'd'}`) ?? []).map((r) => store.kits.get(r.kit_id ?? -1)?.name).join(' + ');
  }

  async function set(performer: Id, song: Id | null, v: string) {
    const body =
      v === 'inherit'
        ? { performer_id: performer, kind, song_id: song, kit_ids: [], inherit: true }
        : v === 'none'
          ? { performer_id: performer, kind, song_id: song, kit_ids: [], inherit: false }
          : { performer_id: performer, kind, song_id: song, kit_ids: [Number(v)], inherit: false };
    const r = await store.act(() => put<{ conflicts: Conflict[] }>(`/api/shows/${store.showId}/assignments/cell`, body));
    if (r?.conflicts.length) store.toast(`Saved with ${r.conflicts.length} conflict(s) in this song — see red cells.`, 'error');
  }

  const conflictTotal = $derived((show?.conflicts.by_song ?? []).reduce((n, s) => n + s.conflicts.length, 0));
</script>

{#if !show}
  <p class="muted">Select or create a show first.</p>
{:else}
  <section class="panel grid">
    <div class="row">
      <h2 class="grow">Assignments</h2>
      {#each ['mic', 'iem', 'guitar'] as const as k (k)}
        <button class:primary={kind === k} onclick={() => (kind = k)}>{k === 'mic' ? 'Mics' : k === 'iem' ? 'IEMs' : 'Guitar'}</button>
      {/each}
      <label class="chk"><input type="checkbox" bind:checked={onlyAssigned} /> Only performers with {kind} kits</label>
    </div>
    <p class="muted small">
      <b>Default</b> applies to every song. A song cell either follows the default (grey), sets a different kit, or sets "none" (off stage for that song).
      {#if conflictTotal}<span class="pill crit">{conflictTotal} conflict(s)</span>{/if}
    </p>
    <div class="table-wrap">
      <table class="grid-table">
        <thead>
          <tr>
            <th class="sticky">Performer</th>
            <th>Default</th>
            {#each songs as s (s.id)}<th title={s.title}>{s.position}. {s.title}</th>{/each}
          </tr>
        </thead>
        <tbody>
          {#each performers as p (p.id)}
            <tr>
              <th class="sticky name">{p.name}</th>
              {#each [null, ...songs.map((s) => s.id)] as song (song ?? 'd')}
                {@const v = value(p.id, song)}
                <td class:inherit={v === 'inherit'} class:none={v === 'none'} class:bad={conflicted.get(String(song ?? 'd'))?.has(p.id)}>
                  <select value={v} disabled={!store.canEdit} onchange={(e) => set(p.id, song, (e.currentTarget as HTMLSelectElement).value)} aria-label="{p.name} {song == null ? 'default' : store.songTitle(song)}">
                    {#if song != null}<option value="inherit">↳ {inheritedLabel(p.id)}</option>{/if}
                    <option value="none">— none —</option>
                    {#if v === 'multi'}<option value="multi" disabled>{multiLabel(p.id, song)}</option>{/if}
                    {#each kitsOfKind as k (k.id)}<option value={String(k.id)}>{k.name}{k.is_spare ? ' (spare)' : ''}</option>{/each}
                  </select>
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if onlyAssigned}<p class="muted small">To assign someone new, untick the filter above.</p>{/if}
  </section>
{/if}

<style>
  .grid { display: grid; gap: 10px; }
  .small { font-size: 0.85rem; margin: 0; }
  .chk { display: flex; align-items: center; gap: 6px; }
  .chk input { min-height: auto; }
  .grid-table th, .grid-table td { white-space: nowrap; padding: 4px; }
  .grid-table thead th { max-width: 140px; overflow: hidden; text-overflow: ellipsis; }
  .sticky { position: sticky; left: 0; background: var(--panel); z-index: 1; }
  .name { text-transform: none; color: var(--text); font-size: 0.95rem; letter-spacing: 0; }
  td select { min-width: 130px; }
  td.inherit select { color: var(--muted); }
  td.none select { color: var(--muted); font-style: italic; }
  td.bad select { border-color: var(--crit); box-shadow: inset 0 0 0 1px var(--crit); }
</style>
