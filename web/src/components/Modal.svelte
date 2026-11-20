<script lang="ts">
  import type { Snippet } from 'svelte';

  let { title, onclose, children }: { title: string; onclose: () => void; children: Snippet } = $props();
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog panel" role="dialog" aria-modal="true" aria-label={title}>
    <div class="head row">
      <h2 class="grow">{title}</h2>
      <button class="ghost big" onclick={onclose} aria-label="Close">✕</button>
    </div>
    {@render children()}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: #000a; display: grid; place-items: center; z-index: 50; padding: 16px; }
  .dialog { width: min(620px, 100%); max-height: calc(100vh - 32px); overflow: auto; display: grid; gap: 12px; }
  .head { position: sticky; top: -12px; background: var(--panel); padding-top: 4px; }
</style>
