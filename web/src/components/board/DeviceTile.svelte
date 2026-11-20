<script lang="ts">
  import { plan, type Tile } from '../../lib/plan.svelte';
  import { store } from '../../lib/store.svelte';

  let { tile, onopen }: { tile: Tile; onopen: (t: Tile) => void } = $props();

  const u = $derived(tile.usage);
  const holder = $derived(tile.holder.kind === 'person' ? plan.person(tile.holder.id) : tile.holder.kind === 'owner' ? 'with owner' : '');
  const line = $derived.by(() => {
    switch (u.usage) {
      case 'in_use': return plan.person(u.by);
      case 'next': return `Next → ${plan.person(u.by)}`;
      case 'collect': return `Collect from ${plan.person(u.from)}`;
      case 'on_spare_for': return `${holder || '—'} · for ${plan.device(u.failed)}`;
      case 'pulled': return `Pulled · ${holder || 'in stock'}`;
      case 'spare': return 'Spare';
      case 'done': return holder ? `Free · ${holder}` : 'Free';
      default: return holder;
    }
  });
  const status = $derived(
    u.usage === 'out' ? 'OUT' : u.usage === 'suspect' ? 'SUSPECT' : u.usage === 'missing' ? 'MISSING' : u.usage === 'pulled' ? 'PULLED' : u.usage === 'on_spare_for' ? 'ON SPARE' : '',
  );
  const bat = $derived(plan.batteries[String(tile.id)]);
  const batText = $derived(
    bat ? `${bat.result === 'low' ? '▲ LOW' : bat.result === 'fresh' ? '✓ fresh' : '✓ OK'} ${new Date(bat.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}` : 'not checked',
  );
  const dim = $derived(u.usage === 'done' || u.usage === 'spare');
  const issue = $derived(u.usage === 'out' || u.usage === 'missing' || u.usage === 'suspect' || u.usage === 'pulled');
</script>

<button
  class="tile"
  class:dim
  class:issue
  class:warn={bat?.result === 'low'}
  data-testid="tile"
  data-device={tile.label}
  data-usage={u.usage}
  onclick={() => onopen(tile)}
>
  <span class="head">
    <b class="label">{tile.label}</b>
    {#if tile.mix}<span class="muted mix">{tile.mix}</span>{/if}
    {#if status}<span class="status">{status}</span>{/if}
  </span>
  <span class="who" title={u.usage === 'next' ? `From ${plan.song(u.song)}` : undefined}>{line || ' '}</span>
  <span class="bat" class:unchecked={!bat && !dim && store.showMode}>{batText}</span>
</button>

<style>
  .tile {
    display: grid; gap: 2px; text-align: left; align-content: start;
    min-height: 88px; padding: 8px 10px; border-radius: 10px;
    background: var(--panel); border: 1px solid var(--line); font-weight: 500;
  }
  .tile.dim { background: color-mix(in srgb, var(--panel) 55%, var(--bg)); }
  .tile.issue { border: 3px solid var(--crit); }
  .tile.warn { border: 3px solid var(--warn); }
  .head { display: flex; gap: 6px; align-items: baseline; }
  .label { font-size: 1.05rem; }
  .mix { font-size: 0.8rem; }
  .status { margin-left: auto; font-size: 0.72rem; font-weight: 800; color: var(--crit); }
  .who { font-size: 0.95rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bat { font-size: 0.78rem; color: var(--muted); }
  .bat.unchecked { color: var(--warn); }
</style>
