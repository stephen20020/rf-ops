<script lang="ts">
  import { store } from '../lib/store.svelte';
  import SongBar from '../components/SongBar.svelte';
  import AlertList from '../components/AlertList.svelte';
  import BatteryBadge from '../components/BatteryBadge.svelte';
  import RaiseAlert from '../components/RaiseAlert.svelte';
  import type { Id, Kit } from '../lib/types';

  let raising = $state(false);
  const show = $derived(store.show);

  /** kit id -> performer ids holding it, for a given effective map */
  function holders(eff: Record<string, Id[]> | null | undefined): Map<Id, Id[]> {
    const m = new Map<Id, Id[]>();
    for (const [p, kits] of Object.entries(eff ?? {})) for (const k of kits) m.set(k, [...(m.get(k) ?? []), Number(p)]);
    return m;
  }
  const nowH = $derived(holders(show?.effective));
  const nextH = $derived(holders(show?.next_effective));

  const channelKits = $derived(
    (show?.kits ?? [])
      .filter((k) => k.kind !== 'iem')
      .sort((a, b) => (a.console_channel ?? 9999) - (b.console_channel ?? 9999) || a.name.localeCompare(b.name)),
  );
  const iemKits = $derived((show?.kits ?? []).filter((k) => k.kind === 'iem').sort((a, b) => (a.iem_mix ?? '').localeCompare(b.iem_mix ?? '', undefined, { numeric: true })));

  function names(ids: Id[] | undefined) {
    return ids?.length ? ids.map((i) => store.performerName(i)).join(', ') : '';
  }
  function kitAlerts(k: Kit) {
    const items = new Set([k.base_id, k.carrier_id, k.element_id].filter((i) => i != null));
    const ps = new Set(nowH.get(k.id) ?? []);
    return store.alerts.filter((a) => (a.equipment_id != null && items.has(a.equipment_id)) || (a.performer_id != null && ps.has(a.performer_id)));
  }
</script>

{#if !show}
  <p class="muted">No show selected.</p>
{:else}
  <div class="layout">
    <div class="main">
      <SongBar compact />
      {#snippet table(kits: Kit[], iem: boolean)}
        <div class="panel table-wrap">
          <table>
            <thead>
              <tr>
                <th>{iem ? 'Mix' : 'Ch'}</th><th>Kit</th><th>Now</th><th>Next</th><th>Pack</th><th>Battery</th><th>Status</th>
              </tr>
            </thead>
            <tbody>
              {#each kits as k (k.id)}
                {@const a = kitAlerts(k)}
                {@const now = names(nowH.get(k.id))}
                {@const next = names(nextH.get(k.id))}
                <tr class:live={!!now} class:crit={a.some((x) => x.severity === 'critical')} class:warnrow={a.length > 0 && !a.some((x) => x.severity === 'critical')} data-testid="foh-row">
                  <td class="ch">{iem ? (k.iem_mix ?? '—') : (k.console_channel ?? '—')}</td>
                  <td>{k.console_channel_name ?? k.name}{#if k.is_spare} <span class="pill">spare</span>{/if}</td>
                  <td class="who">{now || '—'}</td>
                  <td class="muted" class:changes={next !== now}>{next || (show.next_song_id == null ? '' : '—')}</td>
                  <td>{store.label(k.carrier_id)}{#if k.element_id}{' '}<span class="muted">+ {store.label(k.element_id)}</span>{/if}</td>
                  <td><BatteryBadge id={k.carrier_id} /></td>
                  <td>
                    {#each a as x (x.id)}<span class="pill {x.severity === 'critical' ? 'crit' : 'warn'}">{x.title}</span>{/each}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/snippet}
      <h2>Wireless channels</h2>
      {@render table(channelKits, false)}
      <h2>IEM</h2>
      {@render table(iemKits, true)}
    </div>
    <aside class="side panel">
      <div class="row">
        <h2 class="grow">Alerts · {store.alerts.length}</h2>
        <button class="danger" onclick={() => (raising = true)} disabled={!store.connected || !store.canEdit}>Raise</button>
      </div>
      <AlertList alerts={store.alerts} compact />
    </aside>
  </div>
{/if}
{#if raising}<RaiseAlert onclose={() => (raising = false)} />{/if}

<style>
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) 360px; gap: 12px; align-items: start; }
  .main { display: grid; gap: 10px; min-width: 0; }
  .side { display: grid; gap: 8px; position: sticky; top: 64px; }
  td { font-size: 0.92rem; }
  .ch { font-weight: 800; width: 56px; }
  tr:not(.live) td { color: var(--muted); }
  tr.crit { background: color-mix(in srgb, var(--crit) 22%, transparent); }
  tr.warnrow { background: color-mix(in srgb, var(--warn) 14%, transparent); }
  .who { font-weight: 650; }
  .changes { color: var(--warn); }
  @media (max-width: 1000px) {
    .layout { grid-template-columns: 1fr; }
    .side { position: static; order: -1; }
  }
</style>
