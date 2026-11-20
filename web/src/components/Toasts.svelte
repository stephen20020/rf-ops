<script lang="ts">
  import { store } from '../lib/store.svelte';
</script>

<div class="toasts" aria-live="polite">
  {#each store.toasts as t (t.id)}
    <div class="toast {t.kind}" role={t.kind === 'info' ? 'status' : 'alert'}>{t.text}</div>
  {/each}
</div>

<style>
  .toasts { position: fixed; bottom: 16px; left: 50%; transform: translateX(-50%); display: grid; gap: 8px; z-index: 100; width: min(520px, calc(100vw - 32px)); }
  .toast { padding: 12px 14px; border-radius: 10px; background: var(--panel-2); border: 1px solid var(--line); font-weight: 600; box-shadow: 0 6px 24px #0008; }
  .toast.error { border-color: var(--warn); }
  .toast.critical { background: var(--crit); border-color: var(--crit); color: #fff; }
</style>
