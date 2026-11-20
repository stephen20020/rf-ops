<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { del, get, patch, post, put } from '../../lib/api';
  import type { Role } from '../../lib/types';

  let pf = $state({ name: '', role: '', zone: '', owner_user_id: null as number | null });
  let uf = $state({ name: '', role: 'a2' as Role });

  const performers = $derived([...store.performers.values()].sort((a, b) => a.name.localeCompare(b.name)));
  const users = $derived(store.snapshot?.users ?? []);
  const a2s = $derived(users.filter((u) => u.role === 'a2' || u.role === 'admin'));

  async function addPerformer(e: SubmitEvent) {
    e.preventDefault();
    const ok = await store.act(() => post('/api/performers', { ...pf, zone: pf.zone || null }), `Added ${pf.name}`);
    if (ok) pf.name = '';
  }
  async function setPin(id: number, name: string, clear = false) {
    let pin: string | null = null;
    if (!clear) {
      pin = prompt(`New PIN for ${name} (4–8 digits)`);
      if (pin === null) return;
      if (!/^[0-9]{4,8}$/.test(pin)) return store.toast('PIN must be 4–8 digits', 'error');
    }
    // Changing your own PIN signs you out everywhere, including here.
    await store.act(() => put(`/api/users/${id}/pin`, { pin }), clear ? `PIN removed for ${name}` : `PIN set for ${name}`);
  }

  // Helpers: 6-digit join code (3 devices, 2 hours), shown as digits and as a link for a QR.
  let joinCode = $state<{ name: string; code: string; expires_at: string } | null>(null);
  async function makeCode(id: number) {
    const r = await store.act(() => post<{ name: string; code: string; expires_at: string }>('/api/auth/join-codes', { user_id: id }));
    if (r) joinCode = r;
  }
  async function cancelCodes() {
    if (!confirm('Cancel every helper join code? Helpers already signed in stay signed in.')) return;
    await store.act(() => del('/api/auth/join-codes'), 'All join codes cancelled');
    joinCode = null;
  }
  async function signOutEverywhere(id: number, name: string) {
    if (!confirm(`Sign ${name} out on every device?`)) return;
    await store.act(() => post(`/api/users/${id}/sign-out`), `${name} signed out everywhere`);
  }

  // Screens (wall, FOH): enrol THIS browser; the Lead's own sign-in here is replaced.
  type Screen = { id: number; name: string; role: string; last_seen_at: string | null; revoked: boolean };
  let screens = $state<Screen[]>([]);
  let sf = $state({ name: 'Wall', role: 'a2' });
  async function loadScreens() {
    if (!store.isAdmin) return;
    try {
      screens = await get<Screen[]>('/api/auth/screens');
    } catch {
      /* shown elsewhere */
    }
  }
  $effect(() => {
    if (store.authMode === 'pin') void loadScreens();
  });
  async function enrol(e: SubmitEvent) {
    e.preventDefault();
    if (!confirm(`Make this browser the "${sf.name}" screen? You will be signed out here; the screen stays signed in for 7 days (always during a show).`)) return;
    const ok = await store.act(() => post('/api/auth/screens', sf), `This browser is now "${sf.name}"`);
    if (ok) location.reload();
  }
  async function removeScreen(s: Screen) {
    if (!confirm(`Remove screen "${s.name}"? It is signed out at once.`)) return;
    await store.act(() => del(`/api/auth/screens/${s.id}`), `Removed ${s.name}`);
    void loadScreens();
  }

  async function addUser(e: SubmitEvent) {
    e.preventDefault();
    const ok = await store.act(() => post('/api/users', uf), `Added operator ${uf.name}`);
    if (ok) uf.name = '';
  }
</script>

<div class="cols">
  <section class="panel grid">
    <h2>Performers · {performers.length}</h2>
    <form class="row" onsubmit={addPerformer}>
      <input placeholder="Name" bind:value={pf.name} aria-label="Performer name" />
      <input placeholder="Role (e.g. Vocal 2)" bind:value={pf.role} aria-label="Performer role" />
      <input placeholder="Zone" bind:value={pf.zone} aria-label="Zone" />
      <select bind:value={pf.owner_user_id} aria-label="Owning A2"><option value={null}>No A2</option>{#each a2s as u (u.id)}<option value={u.id}>{u.name}</option>{/each}</select>
      <button class="primary" type="submit" disabled={!pf.name.trim()}>Add</button>
    </form>
    <div class="table-wrap">
      <table>
        <thead><tr><th>Name</th><th>Role</th><th>Zone</th><th>A2</th><th></th></tr></thead>
        <tbody>
          {#each performers as p (p.id)}
            <tr>
              <td><strong>{p.name}</strong></td>
              <td><input value={p.role} aria-label="Role of {p.name}" onchange={(e) => store.act(() => patch(`/api/performers/${p.id}`, { role: (e.currentTarget as HTMLInputElement).value }))} /></td>
              <td><input value={p.zone ?? ''} aria-label="Zone of {p.name}" onchange={(e) => store.act(() => patch(`/api/performers/${p.id}`, { zone: (e.currentTarget as HTMLInputElement).value || null }))} /></td>
              <td>
                <select value={p.owner_user_id} aria-label="A2 for {p.name}" onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; store.act(() => patch(`/api/performers/${p.id}`, { owner_user_id: v ? Number(v) : null })); }}>
                  <option value={null}>—</option>
                  {#each a2s as u (u.id)}<option value={u.id}>{u.name}</option>{/each}
                </select>
              </td>
              <td><button class="ghost" onclick={() => confirm(`Delete ${p.name}? Their assignments are removed too.`) && store.act(() => del(`/api/performers/${p.id}`))}>Delete</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </section>

  <section class="panel grid">
    <h2>Operators</h2>
    {#if store.isAdmin}
      <form class="row" onsubmit={addUser}>
        <input placeholder="Name" bind:value={uf.name} aria-label="Operator name" />
        <select bind:value={uf.role} aria-label="Operator role">
          <option value="admin">Lead (admin)</option><option value="a2">a2</option><option value="foh">foh</option><option value="helper">helper (join code)</option><option value="viewer">viewer</option>
        </select>
        <button class="primary" type="submit" disabled={!uf.name.trim()}>Add</button>
      </form>
    {/if}
    <ul class="ops">
      {#each users as u (u.id)}
        <li class="row">
          <span class="grow">{u.name} <span class="pill">{u.role}</span> {#if u.has_pin}🔒{/if}</span>
          {#if store.authMode === 'open'}
            <!-- Open mode: names only, no PINs. -->
          {:else if u.role === 'helper'}
            {#if store.isAdmin}<button class="ghost" onclick={() => makeCode(u.id)}>Join code</button>{/if}
          {:else if store.isAdmin || u.name === store.operatorName}
            <button class="ghost" onclick={() => setPin(u.id, u.name)}>{u.has_pin ? 'Change PIN' : 'Set PIN'}</button>
          {/if}
          {#if store.isAdmin && store.authMode === 'pin'}<button class="ghost" onclick={() => signOutEverywhere(u.id, u.name)}>Sign out</button>{/if}
        </li>
      {/each}
    </ul>
    {#if joinCode}
      <div class="code" data-testid="join-code">
        <div>Code for <b>{joinCode.name}</b>: works on 3 devices for 2 hours</div>
        <div class="digits">{joinCode.code.slice(0, 3)} {joinCode.code.slice(3)}</div>
        <div class="muted small">Or open <code>{location.origin}/#/join?code={joinCode.code}</code></div>
        <button class="ghost" onclick={() => (joinCode = null)}>Hide</button>
      </div>
    {/if}
    {#if store.authMode === 'open'}
      <p class="muted small">Open mode: anyone with the address picks a name on their device; no PINs. Operators listed here
        appear as quick picks, and a <b>viewer</b> name is read-only. PIN sign-in can be turned on later (RFOPS_AUTH=pin).</p>
    {:else}
      <p class="muted small">Everyone except helpers signs in with name + PIN. Helpers use a join code from the Lead.</p>
      {#if store.isAdmin}<button class="ghost" onclick={cancelCodes}>Cancel all join codes</button>{/if}
    {/if}

    {#if store.isAdmin && store.authMode === 'pin'}
      <h2>Screens</h2>
      <form class="row" onsubmit={enrol}>
        <input placeholder="Screen name (Wall SL)" bind:value={sf.name} aria-label="Screen name" />
        <select bind:value={sf.role} aria-label="Screen role">
          <option value="a2">backstage (a2)</option><option value="foh">FOH</option><option value="viewer">view only</option>
        </select>
        <button class="primary" type="submit" disabled={!sf.name.trim()}>Enrol this screen</button>
      </form>
      <ul class="ops">
        {#each screens.filter((x) => !x.revoked) as sc (sc.id)}
          <li class="row">
            <span class="grow">{sc.name} <span class="pill">{sc.role}</span> <span class="muted small">{sc.last_seen_at ? `seen ${new Date(sc.last_seen_at).toLocaleString()}` : 'never seen'}</span></span>
            <button class="ghost" onclick={() => removeScreen(sc)}>Remove</button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .cols { display: grid; grid-template-columns: minmax(0, 2fr) minmax(260px, 1fr); gap: 12px; align-items: start; }
  .grid { display: grid; gap: 10px; }
  .ops { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .small { font-size: 0.85rem; margin: 0; }
  .code { border: 1px solid var(--accent); border-radius: 8px; padding: 10px; display: grid; gap: 6px; }
  .digits { font-size: 2.4rem; font-weight: 700; letter-spacing: 0.1em; font-variant-numeric: tabular-nums; }
  td input { min-width: 100px; }
  @media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
</style>
