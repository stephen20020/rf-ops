<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { patch } from '../../lib/api';
  import Modal from '../Modal.svelte';

  let { onclose }: { onclose: () => void } = $props();

  const show = $derived(store.show!);
  const battery = $derived(show.battery);
  const unchecked = $derived(battery.filter((b) => b.due === 'unchecked').length);
  const overdue = $derived(battery.filter((b) => b.due === 'overdue' || b.due === 'low').length);
  const conflicts = $derived(show.conflicts.by_song.reduce((n, s) => n + s.conflicts.length, 0));
  const spares = $derived(show.kits.filter((k) => k.is_spare).length);

  const MANUAL = [
    'IEM packs (XSW + G4) on, correct channel, volume down, battery checked',
    'Guitar packs (GLXD) charged and paired',
    'Spare packs batteried and on the spare table',
    'A2 Wi-Fi on 5 GHz only (2.4 GHz SSID off — GLX-D uses 2.4 GHz)',
    'Wireless Workbench / dLive show receivers online',
  ];
  let ticks = $state<boolean[]>(MANUAL.map(() => false));

  async function enter() {
    const ok = await store.act(() => patch(`/api/shows/${show.show.id}`, { mode: 'show' }), 'Show mode on — backup taken');
    if (ok) onclose();
  }
</script>

<Modal title="Enter show mode" {onclose}>
  <p class="muted">Show mode locks kits and assignments, takes a backup, and starts automatic battery alerts for packs in use.</p>
  <ul class="auto">
    <li class:bad={conflicts > 0}>{conflicts === 0 ? '✔' : '✖'} {conflicts} assignment conflict{conflicts === 1 ? '' : 's'}</li>
    <li class:bad={unchecked > 0}>{unchecked === 0 ? '✔' : '!'} {unchecked} pack{unchecked === 1 ? '' : 's'} never battery-checked</li>
    <li class:bad={overdue > 0}>{overdue === 0 ? '✔' : '!'} {overdue} pack{overdue === 1 ? '' : 's'} overdue or low</li>
    <li class:bad={store.alerts.length > 0}>{store.alerts.length === 0 ? '✔' : '!'} {store.alerts.length} active alert{store.alerts.length === 1 ? '' : 's'}</li>
    <li class:bad={spares === 0}>{spares > 0 ? '✔' : '!'} {spares} spare kit{spares === 1 ? '' : 's'}</li>
  </ul>
  <div class="manual">
    {#each MANUAL as m, i (m)}
      <label class="chk"><input type="checkbox" bind:checked={ticks[i]} /> {m}</label>
    {/each}
  </div>
  <div class="row">
    <button class="danger big" onclick={enter} data-testid="confirm-show-mode">Enter show mode{ticks.every(Boolean) ? '' : ' anyway'}</button>
    <button class="big" onclick={onclose}>Cancel</button>
  </div>
</Modal>

<style>
  .auto { list-style: none; padding: 0; margin: 0; display: grid; gap: 6px; font-weight: 600; }
  .auto li { color: var(--ok); }
  .auto li.bad { color: var(--warn); }
  .manual { display: grid; gap: 8px; }
  .chk { display: flex; gap: 10px; align-items: center; color: var(--text); font-size: 0.95rem; }
  .chk input { min-height: auto; width: 22px; height: 22px; flex: none; }
</style>
