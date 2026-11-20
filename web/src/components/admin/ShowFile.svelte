<script lang="ts">
  // Show files (docs/show-files.md): paste or drop a YAML file, check it, import it; export any show.
  import { store } from '../../lib/store.svelte';
  import { deviceId, nameHeader } from '../../lib/api';

  type Problem = { level: string; at: string; message: string };
  let text = $state('');
  let result = $state<{ ok: boolean; errors: Problem[]; warnings: Problem[]; show_id?: number } | null>(null);
  let busy = $state(false);

  async function send(path: string) {
    busy = true;
    try {
      const r = await fetch(path, {
        method: 'POST',
        headers: { 'content-type': 'text/yaml', 'x-rfops-client': deviceId, ...nameHeader() },
        body: text,
      });
      result = await r.json();
      if (r.ok && path.includes('import') && result?.show_id) {
        store.toast('Show imported');
        store.selectShow(result.show_id);
      }
    } catch (e) {
      store.toast(String(e), 'error');
    } finally {
      busy = false;
    }
  }
  async function exportShow() {
    if (!store.showId) return;
    const r = await fetch(`/api/shows/${store.showId}/showfile`, { headers: nameHeader() });
    text = await r.text();
    result = null;
  }
  function download() {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: 'text/yaml' }));
    a.download = `${(store.show?.show.name ?? 'show').replace(/[^\w-]+/g, '-')}.yaml`;
    a.click();
  }
  async function drop(e: DragEvent) {
    e.preventDefault();
    const f = e.dataTransfer?.files[0];
    if (f) text = await f.text();
  }
</script>

<section class="panel grid">
  <h2>Show file</h2>
  <p class="muted small">
    A whole show in one readable file: people, songs, who has which device, passes, mixes. Write it by hand, edit an
    exported one, or have an AI agent write it from the band's documents (see <code>docs/show-files.md</code>).
    Command line: <code>rfops show check|import|export</code>.
  </p>
  <div class="row">
    <button onclick={exportShow} disabled={!store.showId}>Load "{store.show?.show.name ?? '—'}" into the editor</button>
    <button onclick={download} disabled={!text}>Download</button>
  </div>
  <textarea
    bind:value={text}
    ondragover={(e) => e.preventDefault()}
    ondrop={drop}
    rows="18"
    spellcheck="false"
    placeholder="Paste a show file here, or drop a .yaml file"
    data-testid="showfile-text"
  ></textarea>
  <div class="row">
    <button onclick={() => send('/api/showfile/check')} disabled={busy || !text}>Check</button>
    <button class="primary" onclick={() => send('/api/showfile/import')} disabled={busy || !text} data-testid="showfile-import">Import</button>
  </div>
  {#if result}
    <div class="results" data-testid="showfile-result">
      {#each result.errors as p, i (i)}<div class="err">✗ <b>{p.at}</b>: {p.message}</div>{/each}
      {#each result.warnings as p, i (i)}<div class="warn">! <b>{p.at}</b>: {p.message}</div>{/each}
      {#if result.ok}<div class="ok">✓ No errors{result.show_id ? ' — imported.' : '.'}</div>{:else}<div class="muted">Nothing was changed.</div>{/if}
    </div>
  {/if}
</section>

<style>
  .grid { display: grid; gap: 8px; }
  .small { font-size: 0.85rem; }
  textarea { font-family: ui-monospace, Menlo, monospace; font-size: 0.85rem; width: 100%; }
  .results { display: grid; gap: 4px; }
  .err { color: var(--crit); }
  .warn { color: var(--warn); }
  .ok { color: var(--ok); }
</style>
