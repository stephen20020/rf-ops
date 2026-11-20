<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { del, patch, post } from '../../lib/api';
  import { consoleTag, KIND_LABEL } from '../../lib/format';
  import ShowModeChecklist from './ShowModeChecklist.svelte';
  import type { EquipmentKind, KitKind, Show } from '../../lib/types';

  const SLOTS: Record<KitKind, { base: EquipmentKind[]; carrier: EquipmentKind[]; element: EquipmentKind[] }> = {
    mic: { base: ['receiver_channel'], carrier: ['handheld_tx', 'bodypack_tx'], element: ['capsule', 'headset', 'lav'] },
    iem: { base: ['iem_tx_channel', 'iem_tx'], carrier: ['iem_pack'], element: [] },
    guitar: { base: ['guitar_receiver'], carrier: ['guitar_pack'], element: [] },
  };

  const show = $derived(store.show);
  let newShow = $state('');
  let checklist = $state(false);
  let newSong = $state('');
  let kit = $state({ kind: 'mic' as KitKind, name: '', base_id: null as number | null, carrier_id: null as number | null, element_id: null as number | null, console_channel: '', dca: '', iem_mix: '', is_spare: false });

  const opts = (kinds: EquipmentKind[]) =>
    [...store.equipment.values()].filter((e) => kinds.includes(e.kind)).sort((a, b) => a.label.localeCompare(b.label, undefined, { numeric: true }));

  function toLocal(iso: string | null): string {
    if (!iso) return '';
    const d = new Date(iso);
    const p = (n: number) => String(n).padStart(2, '0');
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
  }
  const fromLocal = (v: string) => (v ? new Date(v).toISOString() : null);

  async function createShow(e: SubmitEvent) {
    e.preventDefault();
    const s = await store.act(() => post<Show>('/api/shows', { name: newShow.trim() }), 'Show created');
    if (s) {
      newShow = '';
      store.selectShow(s.id);
    }
  }
  async function duplicate() {
    if (!show) return;
    const name = prompt('Name for the copy', `${show.show.name} (copy)`);
    if (!name) return;
    const s = await store.act(() => post<Show>(`/api/shows/${show.show.id}/duplicate`, { name }), 'Show duplicated');
    if (s) store.selectShow(s.id);
  }
  async function addSong(e: SubmitEvent) {
    e.preventDefault();
    const ok = await store.act(() => post(`/api/shows/${show!.show.id}/songs`, { title: newSong.trim() }));
    if (ok) newSong = '';
  }
  function move(i: number, d: number) {
    const ids = store.songs.map((s) => s.id);
    const j = i + d;
    if (j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    return store.act(() => post(`/api/shows/${show!.show.id}/songs/reorder`, { song_ids: ids }));
  }
  function pcValue(trigger: string | null): string {
    try {
      return trigger ? String(JSON.parse(trigger).value ?? '') : '';
    } catch {
      return '';
    }
  }
  async function addKit(e: SubmitEvent) {
    e.preventDefault();
    const ok = await store.act(
      () =>
        post(`/api/shows/${show!.show.id}/kits`, {
          ...kit,
          console_channel: kit.console_channel ? Number(kit.console_channel) : null,
          console_channel_name: kit.console_channel ? kit.name : null,
          dca: kit.dca || null,
          iem_mix: kit.iem_mix || null,
          element_id: SLOTS[kit.kind].element.length ? kit.element_id : null,
        }),
      `Kit ${kit.name} added`,
    );
    if (ok) kit = { ...kit, name: '', carrier_id: null, element_id: null, base_id: null, console_channel: '' };
  }
</script>

{#if checklist && show}<ShowModeChecklist onclose={() => (checklist = false)} />{/if}

<div class="cols">
  <section class="panel grid">
    <h2>Shows</h2>
    <form class="row" onsubmit={createShow}>
      <input class="grow" placeholder="New show name" bind:value={newShow} aria-label="New show name" />
      <button class="primary" type="submit" disabled={!newShow.trim()}>Create</button>
    </form>
    {#if show}
      {@const s = show.show}
      <label>Name<input value={s.name} onchange={(e) => store.act(() => patch(`/api/shows/${s.id}`, { name: (e.currentTarget as HTMLInputElement).value }))} /></label>
      <label>Call time (battery checks due after this)
        <input type="datetime-local" value={toLocal(s.call_time)} onchange={(e) => store.act(() => patch(`/api/shows/${s.id}`, { call_time: fromLocal((e.currentTarget as HTMLInputElement).value) }))} />
      </label>
      <div class="grid modes">
        <span class="muted small">Mode</span>
        <div class="row">
          {#each ['setup', 'rehearsal'] as const as m (m)}
            <button class:primary={s.mode === m} onclick={() => store.act(() => patch(`/api/shows/${s.id}`, { mode: m }))}>{m}</button>
          {/each}
          <button class:danger={s.mode === 'show'} onclick={() => (s.mode === 'show' ? null : (checklist = true))} data-testid="enter-show-mode">{s.mode === 'show' ? 'SHOW MODE' : 'Enter show mode…'}</button>
        </div>
      </div>
      <div class="row">
        <button onclick={duplicate}>Duplicate as template</button>
        <button class="ghost" onclick={() => confirm(`Delete show "${s.name}"?`) && store.act(() => del(`/api/shows/${s.id}`)).then(() => store.selectShow(null))}>Delete show</button>
      </div>
    {/if}
  </section>

  {#if show}
    <section class="panel grid">
      <h2>Setlist · {store.songs.length}</h2>
      <form class="row" onsubmit={addSong}>
        <input class="grow" placeholder="Song or segment" bind:value={newSong} aria-label="New song" />
        <button class="primary" type="submit" disabled={!newSong.trim()}>Add</button>
      </form>
      <ol class="songs">
        {#each store.songs as s, i (s.id)}
          <li class:skipped={s.skipped} class:current={s.id === show.show.current_song_id}>
            <span class="grow">{s.title}</span>
            <label class="pc" title="MIDI Program Change value that selects this song">PC
              <input type="number" min="0" max="127" value={pcValue(s.midi_trigger)} onchange={(e) => { const v = (e.currentTarget as HTMLInputElement).value; store.act(() => patch(`/api/songs/${s.id}`, { midi_trigger: v === '' ? null : { type: 'program_change', channel: 16, value: Number(v) } })); }} />
            </label>
            <button class="ghost" onclick={() => move(i, -1)} disabled={i === 0} aria-label="Move up">↑</button>
            <button class="ghost" onclick={() => move(i, 1)} disabled={i === store.songs.length - 1} aria-label="Move down">↓</button>
            <button class="ghost" onclick={() => store.act(() => patch(`/api/songs/${s.id}`, { skipped: !s.skipped }))}>{s.skipped ? 'Unskip' : 'Skip'}</button>
            <button class="ghost" onclick={() => confirm(`Delete "${s.title}"?`) && store.act(() => del(`/api/songs/${s.id}`))} aria-label="Delete">✕</button>
          </li>
        {/each}
      </ol>
    </section>

    <section class="panel grid wide">
      <h2>Kits · {show.kits.length}</h2>
      <form class="row" onsubmit={addKit} inert={store.showMode} class:locked={store.showMode}>
        <label>Type<select bind:value={kit.kind}><option value="mic">Mic</option><option value="iem">IEM</option><option value="guitar">Guitar</option></select></label>
        <label>Name *<input bind:value={kit.name} placeholder="Vox 5" /></label>
        <label>Receiver / TX ch<select bind:value={kit.base_id}><option value={null}>—</option>{#each opts(SLOTS[kit.kind].base) as e (e.id)}<option value={e.id}>{e.label}</option>{/each}</select></label>
        <label>Pack<select bind:value={kit.carrier_id}><option value={null}>—</option>{#each opts(SLOTS[kit.kind].carrier) as e (e.id)}<option value={e.id}>{e.label}</option>{/each}</select></label>
        {#if SLOTS[kit.kind].element.length}
          <label>Capsule / element<select bind:value={kit.element_id}><option value={null}>—</option>{#each opts(SLOTS[kit.kind].element) as e (e.id)}<option value={e.id}>{e.label}</option>{/each}</select></label>
        {/if}
        {#if kit.kind === 'iem'}
          <label>Mix<input bind:value={kit.iem_mix} placeholder="Aux 5" /></label>
        {:else}
          <label>Console ch<input type="number" min="1" bind:value={kit.console_channel} /></label>
          <label>DCA<input bind:value={kit.dca} /></label>
        {/if}
        <label class="chk"><input type="checkbox" bind:checked={kit.is_spare} /> Spare</label>
        <button class="primary" type="submit" disabled={!kit.name.trim()}>Add kit</button>
      </form>
      <div class="table-wrap">
        <table>
          <thead><tr><th>Type</th><th>Name</th><th>Console</th><th>Base</th><th>Pack</th><th>Element</th><th></th></tr></thead>
          <tbody>
            {#each [...show.kits].sort((a, b) => a.kind.localeCompare(b.kind) || (a.console_channel ?? 999) - (b.console_channel ?? 999)) as k (k.id)}
              <tr>
                <td>{KIND_LABEL[k.kind]}{#if k.is_spare} <span class="pill">spare</span>{/if}</td>
                <td><strong>{k.name}</strong></td>
                <td>{consoleTag(k)}{#if k.dca} · DCA {k.dca}{/if}</td>
                <td>{store.label(k.base_id)}</td>
                <td>{store.label(k.carrier_id)}</td>
                <td>{k.element_id ? store.label(k.element_id) : ''}</td>
                <td><button class="ghost" disabled={store.showMode} onclick={() => confirm(`Delete kit ${k.name}? Its assignments are removed too.`) && store.act(() => del(`/api/kits/${k.id}`))}>Delete</button></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {/if}
</div>

<style>
  .cols { display: grid; grid-template-columns: minmax(260px, 1fr) minmax(0, 2fr); gap: 12px; align-items: start; }
  .wide { grid-column: 1 / -1; }
  .grid { display: grid; gap: 10px; }
  .songs { margin: 0; padding-left: 22px; display: grid; gap: 4px; }
  .songs li { display: flex; gap: 6px; align-items: center; }
  .songs li.skipped .grow { text-decoration: line-through; color: var(--muted); }
  .songs li.current .grow { color: var(--accent); font-weight: 700; }
  .pc { display: flex; align-items: center; gap: 4px; }
  .pc input { width: 70px; }
  .chk { display: flex; align-items: center; gap: 6px; }
  .locked { opacity: 0.4; }
  .modes .small { font-size: 0.85rem; }
  .modes button { text-transform: capitalize; }
  .chk input { min-height: auto; }
  @media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
</style>
