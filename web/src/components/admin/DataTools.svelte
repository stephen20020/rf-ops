<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { get, post, postText } from '../../lib/api';
  import type { BackupFile, Show } from '../../lib/types';

  let backups = $state<{ enabled: boolean; dir?: string; files: BackupFile[] } | null>(null);
  let importMsg = $state<{ ok: boolean; text: string; errors?: string[] } | null>(null);

  async function loadBackups() {
    try {
      backups = await get('/api/backups');
    } catch (e) {
      store.toast(String(e), 'error');
    }
  }
  $effect(() => {
    void loadBackups();
  });

  async function backupNow() {
    const r = await store.act(() => post<BackupFile>('/api/backups'), 'Backup written');
    if (r) await loadBackups();
  }

  async function readFile(e: Event): Promise<string | null> {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    return f ? await f.text() : null;
  }

  async function importCsv(e: Event, what: 'equipment' | 'performers') {
    const text = await readFile(e);
    if (text == null) return;
    if (!store.connected || !store.canEdit) return store.toast('Cannot import right now (offline or read-only).', 'error');
    const r = await postText(`/api/import/${what}`, text);
    if (r.status === 200) {
      importMsg = { ok: true, text: `${what}: ${r.json.created} created, ${r.json.updated} updated` };
    } else {
      importMsg = { ok: false, text: `${what} import rejected — nothing was changed`, errors: r.json?.errors ?? [r.json?.error ?? `HTTP ${r.status}`] };
    }
  }

  async function importShow(e: Event) {
    const text = await readFile(e);
    if (text == null) return;
    let file: unknown;
    try {
      file = JSON.parse(text);
    } catch {
      importMsg = { ok: false, text: 'That file is not valid JSON.' };
      return;
    }
    const r = await store.act(() => post<{ show: Show; performers_created: string[] }>('/api/shows/import', { file }));
    if (r) {
      importMsg = {
        ok: true,
        text: `Imported "${r.show.name}"${r.performers_created.length ? `; new performers: ${r.performers_created.join(', ')}` : ''}`,
      };
      store.selectShow(r.show.id);
    }
  }

  const kb = (n: number) => `${Math.round(n / 1024)} KB`;
</script>

<div class="cols">
  <section class="panel grid">
    <h2>Spreadsheets (CSV)</h2>
    <p class="muted small">Export, edit in Excel/Numbers/Sheets, import back. Rows match on <b>label</b> (equipment) or <b>name</b> (performers): existing rows update, new rows are added. Any error rejects the whole file.</p>
    <div class="row">
      <a class="btn link" href="/api/export/equipment.csv" download>Export equipment</a>
      <label class="btn link">Import equipment<input type="file" accept=".csv,text/csv" onchange={(e) => importCsv(e, 'equipment')} hidden /></label>
    </div>
    <div class="row">
      <a class="btn link" href="/api/export/performers.csv" download>Export performers</a>
      <label class="btn link">Import performers<input type="file" accept=".csv,text/csv" onchange={(e) => importCsv(e, 'performers')} hidden /></label>
    </div>
    {#if store.showId}
      <div class="row"><a class="btn link" href={`/api/shows/${store.showId}/export/plot.csv`} download>Export mic plot (this show)</a></div>
    {/if}
  </section>

  <section class="panel grid">
    <h2>Shows</h2>
    <p class="muted small">A show file carries the setlist, kits and assignments. Equipment is matched by label, so import into a system with the same inventory.</p>
    <div class="row">
      {#if store.showId}<a class="btn link" href={`/api/shows/${store.showId}/export`} download>Export this show</a>{/if}
      <label class="btn link">Import show file<input type="file" accept=".json,application/json" onchange={importShow} hidden /></label>
    </div>
    {#if store.showId}<div class="row"><a class="btn link" href="#/report">Post-show report</a></div>{/if}
  </section>

  {#if importMsg}
    <section class="panel msg" class:bad={!importMsg.ok}>
      <strong>{importMsg.text}</strong>
      {#if importMsg.errors?.length}<ul>{#each importMsg.errors as e, i (i)}<li>{e}</li>{/each}</ul>{/if}
      <button class="ghost" onclick={() => (importMsg = null)}>Dismiss</button>
    </section>
  {/if}

  <section class="panel grid wide">
    <div class="row">
      <h2 class="grow">Backups</h2>
      <button class="primary" onclick={backupNow} disabled={!backups?.enabled}>Back up now</button>
    </div>
    {#if backups && !backups.enabled}
      <p class="muted">Backups are off (in-memory database).</p>
    {:else if backups}
      <p class="muted small">Automatic every 15 minutes (when something changed) and when a show enters show mode. Folder: <code>{backups.dir}</code>. To restore: stop the server, copy a backup over the database file, start the server.</p>
      <div class="table-wrap">
        <table>
          <thead><tr><th>File</th><th>Size</th><th>Created</th></tr></thead>
          <tbody>{#each backups.files.slice(0, 15) as f (f.name)}<tr><td><code>{f.name}</code></td><td>{kb(f.bytes)}</td><td>{new Date(f.created_at).toLocaleString()}</td></tr>{:else}<tr><td colspan="3" class="muted">No backups yet.</td></tr>{/each}</tbody>
        </table>
      </div>
    {/if}
  </section>
</div>

<style>
  .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; align-items: start; }
  .wide, .msg { grid-column: 1 / -1; }
  .grid { display: grid; gap: 10px; }
  .small { font-size: 0.85rem; margin: 0; }
  .link { display: inline-flex; align-items: center; text-decoration: none; color: var(--text); cursor: pointer; }
  .msg { border-color: var(--ok); }
  .msg.bad { border-color: var(--crit); }
  .msg ul { margin: 6px 0; }
  @media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
</style>
