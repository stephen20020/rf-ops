<script lang="ts">
  import { store } from '../lib/store.svelte';
  import Inventory from '../components/admin/Inventory.svelte';
  import People from '../components/admin/People.svelte';
  import ShowEditor from '../components/admin/ShowEditor.svelte';
  import AssignmentGrid from '../components/admin/AssignmentGrid.svelte';
  import DataTools from '../components/admin/DataTools.svelte';
  import MidiSetup from '../components/admin/MidiSetup.svelte';

  const TABS = ['Assignments', 'Show & kits', 'MIDI', 'Inventory', 'People', 'Data'] as const;
  type Tab = (typeof TABS)[number];
  let tab = $state<Tab>('Assignments');

  const conflictCount = $derived(
    (store.show?.conflicts.by_song ?? []).reduce((n, s) => n + s.conflicts.length, 0) +
      (store.show?.conflicts.static_conflicts.length ?? 0),
  );
</script>

<div class="admin">
  <div class="row tabs" role="tablist">
    {#each TABS as t (t)}
      <button role="tab" aria-selected={tab === t} class:primary={tab === t} onclick={() => (tab = t)}>
        {t}{#if t === 'Assignments' && conflictCount} <span class="pill crit">{conflictCount}</span>{/if}
      </button>
    {/each}
    {#if !store.canEdit}<span class="muted">Read-only: your role cannot edit.</span>{/if}
  </div>
  {#if store.showMode}
    <div class="lock" data-testid="show-lock">🔒 <b>Show mode:</b> kits and assignments are locked. Use <b>Swap to spare</b> backstage, or switch the show back to rehearsal in Show &amp; kits to edit.</div>
  {/if}
  {#if tab === 'Inventory'}
    <Inventory />
  {:else if tab === 'People'}
    <People />
  {:else if tab === 'MIDI'}
    <MidiSetup />
  {:else if tab === 'Data'}
    <DataTools />
  {:else if tab === 'Show & kits'}
    <ShowEditor />
  {:else}
    <AssignmentGrid />
  {/if}
</div>

<style>
  .admin { display: grid; gap: 12px; }
  .lock { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--crit); background: color-mix(in srgb, var(--crit) 12%, var(--panel)); }
</style>
