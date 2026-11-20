<script lang="ts">
  // Standby runbook step 3 (docs/reliability.md §3.6). Opened on the standby's own address.
  type Status = {
    fenced: boolean;
    r2: boolean;
    lease: { instance: string; holder: string | null; generation: number; reason: string } | null;
    newest_backup: { key?: string; age_s?: number | null; error?: string } | null;
  };
  let st = $state<Status | null>(null);
  let err = $state('');
  let name = $state('');
  let pin = $state('');
  let busy = $state(false);
  let done = $state<{ restored_from: string; generation: number; next: string } | null>(null);

  async function load() {
    try {
      const r = await fetch('/api/standby/status');
      st = await r.json();
    } catch (e) {
      err = String(e);
    }
  }
  void load();

  const age = $derived(st?.newest_backup?.age_s ?? null);
  const old = $derived(age !== null && age > 15 * 60);

  async function takeOver(e: SubmitEvent) {
    e.preventDefault();
    if (!confirm('Take over? This server becomes the active RF Ops and the other one is shut out.')) return;
    busy = true;
    err = '';
    try {
      const r = await fetch('/api/standby/takeover', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name, pin }),
      });
      const j = await r.json();
      if (!r.ok) err = j.error ?? r.statusText;
      else done = j;
    } catch (e2) {
      err = String(e2);
    } finally {
      busy = false;
      pin = '';
    }
  }
</script>

<section class="tk">
  <h1>RF Ops — Take over</h1>
  {#if !st}
    <p class="muted">Checking…</p>
  {:else if !st.r2}
    <p class="warnbox">This server has no R2 configured, so it can't take over.</p>
  {:else if !st.fenced && !done}
    <p>This server is already the <b>active</b> RF Ops (lease generation {st.lease?.generation}). Nothing to do.</p>
  {:else if done}
    <p class="okbox" data-testid="takeover-done">
      Taken over from <code>{done.restored_from}</code> (generation {done.generation}). {done.next}
    </p>
    <ol>
      <li>Wait about 30 seconds for the restart.</li>
      <li>Cloudflare → Workers → <b>rfops-proxy</b> → set <code>ORIGIN</code> to <code>{location.host}</code>.</li>
      <li>Check that the wall and FOH screens reconnect.</li>
    </ol>
  {:else}
    <p>Lease is held by <b>{st.lease?.holder ?? 'nobody'}</b> (generation {st.lease?.generation}). {st.lease?.reason}</p>
    <ol>
      <li>Render: <b>Suspend</b> the old <code>rfops</code> service if it is still running.</li>
      <li>Enter a Lead's name and PIN below.</li>
    </ol>
    {#if st.newest_backup?.error}
      <p class="warnbox">Can't read R2: {st.newest_backup.error}. Take over needs R2; use the paper plot until it is back.</p>
    {:else if st.newest_backup?.key}
      <p class:warnbox={old}>
        Will restore <code>{st.newest_backup.key}</code>{#if age !== null}, {Math.round(age / 60)} min old{/if}.
        {#if old}Older than 15 min: changes since then can't all be recovered by the screens; check the plot after.{/if}
      </p>
    {:else}
      <p class="warnbox">No backups in R2. Take over isn't possible.</p>
    {/if}
    <form onsubmit={takeOver}>
      <input placeholder="Lead name" bind:value={name} autocomplete="username" />
      <input placeholder="PIN" bind:value={pin} inputmode="numeric" type="password" autocomplete="current-password" />
      <button class="primary big" disabled={busy || !name || pin.length < 4 || !st.newest_backup?.key}>Take over</button>
    </form>
  {/if}
  {#if err}<p class="warnbox">{err}</p>{/if}
</section>

<style>
  .tk { max-width: 560px; margin: 5vh auto; padding: 16px; display: grid; gap: 12px; }
  form { display: grid; gap: 8px; }
  input { min-height: 48px; font-size: 1.1rem; }
  .warnbox { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--warn); color: var(--warn); margin: 0; }
  .okbox { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--accent); margin: 0; }
</style>
