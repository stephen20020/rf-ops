<script lang="ts">
  import { plan } from '../../lib/plan.svelte';
</script>

{#if plan.undo}
  <div class="undo" data-testid="undo-bar">
    <span class="grow">✓ {plan.undo.text}</span>
    <button class="big" onclick={() => plan.doUndo()}>Undo</button>
  </div>
{/if}

<style>
  .undo {
    position: fixed; left: 50%; bottom: 16px; transform: translateX(-50%); z-index: 40;
    display: flex; gap: 12px; align-items: center; width: min(560px, calc(100% - 32px));
    padding: 8px 8px 8px 14px; border-radius: 12px; background: var(--panel-2); border: 1px solid var(--accent);
  }
</style>
