<script lang="ts">
  import { store } from '../lib/store.svelte';
  import { DUE_LABEL } from '../lib/format';
  import type { Id } from '../lib/types';

  let { id }: { id: Id | null } = $props();
  const b = $derived(id == null ? undefined : store.battery.get(id));
  const cls = $derived(!b ? '' : b.due === 'ok' ? 'ok' : b.due === 'due_soon' ? 'warn' : b.due === 'low' || b.due === 'overdue' ? 'crit' : 'warn');
</script>

{#if b}
  <span class="pill {cls}" title={b.last_checked_at ? `${b.last_result} by ${b.last_checked_by}` : 'Never checked'}>🔋 {DUE_LABEL[b.due]}</span>
{/if}
