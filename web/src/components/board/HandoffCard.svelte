<script lang="ts">
  import { plan, type Handoff } from '../../lib/plan.svelte';

  let { h, overdue = false }: { h: Handoff; overdue?: boolean } = $props();
  const hint = $derived(plan.hint(h));
</script>

<div class="card" class:overdue data-testid="handoff-card">
  <div class="grow">
    <div class="what">{plan.text(h)}</div>
    {#if hint}<div class="muted hint">{hint}</div>{/if}
    {#if h.via_sub}<div class="muted hint">on a spare (swap)</div>{/if}
  </div>
  {#if h.action.type !== 'keep'}
    {#if overdue}<button class="ghost" onclick={() => plan.leave(h)}>Leave it</button>{/if}
    <button class="primary done" onclick={() => plan.done(h)}>Done</button>
  {/if}
</div>

<style>
  .card { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--line); }
  .card.overdue { border: 2px solid var(--warn); }
  .what { font-weight: 650; }
  .hint { font-size: 0.8rem; }
  .done { min-height: 64px; min-width: 88px; font-size: 1.1rem; }
</style>
