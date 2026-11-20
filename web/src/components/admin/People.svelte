<script lang="ts">
  import { store } from '../../lib/store.svelte';
  import { del, patch, post } from '../../lib/api';
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
          <option value="admin">admin</option><option value="a2">a2</option><option value="foh">foh</option><option value="viewer">viewer</option>
        </select>
        <button class="primary" type="submit" disabled={!uf.name.trim()}>Add</button>
      </form>
    {/if}
    <ul>{#each users as u (u.id)}<li>{u.name} <span class="pill">{u.role}</span></li>{/each}</ul>
  </section>
</div>

<style>
  .cols { display: grid; grid-template-columns: minmax(0, 2fr) minmax(260px, 1fr); gap: 12px; align-items: start; }
  .grid { display: grid; gap: 10px; }
  ul { margin: 0; padding-left: 18px; display: grid; gap: 4px; }
  td input { min-width: 100px; }
  @media (max-width: 900px) { .cols { grid-template-columns: 1fr; } }
</style>
