// End-to-end tests for Phase 2: two live clients, alert lifecycle, song advance, server restart,
// touch target size, viewer role. Usage (from web/): npm run build && npm run test:e2e
// Requires the server binary: cargo build -p rfops-server (uses ../target/debug/rfops-server).

const { chromium } = require('playwright');
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const net = require('node:net');
const assert = require('node:assert/strict');

const ROOT = path.resolve(__dirname, '..', '..');
const BIN = process.env.RFOPS_BIN || path.join(ROOT, 'target', 'debug', 'rfops-server');
const WEB = path.join(ROOT, 'web', 'dist');

function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

async function startServer(db, port, seed) {
  const args = seed ? ['--seed-demo'] : [];
  const proc = spawn(BIN, args, {
    env: {
      ...process.env,
      RFOPS_DB: `sqlite://${db}`,
      RFOPS_ADDR: `127.0.0.1:${port}`,
      RFOPS_WEB: WEB,
      RUST_LOG: 'warn',
      RFOPS_ADMIN_NAME: 'Admin',
      RFOPS_ADMIN_PIN: PINS.Admin,
    },
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/health`);
      if (r.ok) return proc;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  proc.kill();
  throw new Error('server did not start');
}

function stopServer(proc) {
  return new Promise((resolve) => {
    proc.once('exit', resolve);
    proc.kill('SIGKILL');
  });
}

// Every operator signs in with a PIN (step 1b). The Lead's comes from RFOPS_ADMIN_PIN; the
// others are set by the Lead at the start of the run.
const PINS = { Admin: '246810' };
const pinFor = (name) => PINS[name] ?? '1111';
const sessions = {};

async function login(base, name) {
  const r = await fetch(`${base}/api/auth/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-rfops-client': `e2e-node-${name}` },
    body: JSON.stringify({ name, pin: pinFor(name) }),
  });
  assert.equal(r.status, 200, `sign in ${name}: ${await r.clone().text()}`);
  sessions[name] = r.headers.getSetCookie().find((c) => c.startsWith('rfops_session=')).split(';')[0];
  return sessions[name];
}

/** API call signed in as `name` (from Node, no Origin header). */
async function api(base, name, method, path, body) {
  const cookie = sessions[name] ?? (await login(base, name));
  return fetch(`${base}${path}`, {
    method,
    headers: { 'content-type': 'application/json', cookie },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

async function client(browser, base, operator, hash, viewport = { width: 1920, height: 1080 }) {
  const ctx = await browser.newContext({ viewport });
  // Sign in through the browser context so it holds its own session cookie.
  const r = await ctx.request.post(`${base}/api/auth/login`, { data: { name: operator, pin: pinFor(operator) } });
  assert.equal(r.status(), 200, `browser sign in ${operator}`);
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(`${base}/${hash}`);
  await page.getByTestId('connection').filter({ hasText: 'Live' }).waitFor({ timeout: 5000 });
  return { ctx, page, errors };
}

const results = [];
async function test(name, fn) {
  const t0 = Date.now();
  try {
    await fn();
    results.push({ name, ok: true, ms: Date.now() - t0 });
    console.log(`  ✓ ${name} (${Date.now() - t0} ms)`);
  } catch (e) {
    results.push({ name, ok: false, error: e });
    console.log(`  ✗ ${name}\n    ${e && e.stack ? e.stack.split('\n').slice(0, 4).join('\n    ') : e}`);
  }
}

async function timed(fn) {
  const t0 = Date.now();
  await fn();
  return Date.now() - t0;
}

(async () => {
  if (!fs.existsSync(BIN)) throw new Error(`server binary not found at ${BIN}; run cargo build -p rfops-server`);
  if (!fs.existsSync(path.join(WEB, 'index.html'))) throw new Error('web/dist missing; run npm run build');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rfops-e2e-'));
  const db = path.join(dir, 'e2e.db');
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  let server = await startServer(db, port, true);
  const browser = await chromium.launch();
  const users = await (await api(base, 'Admin', 'GET', '/api/users')).json();
  for (const u of users.filter((u) => u.name !== 'Admin')) {
    const r = await api(base, 'Admin', 'PUT', `/api/users/${u.id}/pin`, { pin: pinFor(u.name) });
    assert.equal(r.status, 200);
  }

  const back = await client(browser, base, 'A2 Stage Left', '#/backstage');
  const foh = await client(browser, base, 'FOH', '#/foh', { width: 1440, height: 900 });
  const right = await client(browser, base, 'A2 Stage Right', '#/backstage');

  console.log('RF Ops e2e');

  await test('alert raised backstage appears at FOH in < 1 s', async () => {
    await back.page.getByTestId('raise-alert').click();
    await back.page.getByRole('button', { name: 'Dropout' }).click();
    const fohAlert = foh.page.getByTestId('alert').filter({ hasText: 'Dropout' });
    const ms = await timed(async () => {
      await back.page.getByRole('button', { name: 'Raise alert', exact: true }).last().click();
      await fohAlert.waitFor({ timeout: 1000 });
    });
    assert.ok(ms < 1000, `took ${ms} ms`);
    assert.equal(await fohAlert.getAttribute('data-state'), 'open');
  });

  await test('ack from FOH shows on both backstage screens in < 1 s', async () => {
    const ms = await timed(async () => {
      await foh.page.getByTestId('alert').filter({ hasText: 'Dropout' }).getByRole('button', { name: 'Ack' }).click();
      for (const c of [back, right]) {
        await c.page.locator('[data-testid=alert][data-state=acknowledged]').filter({ hasText: 'Dropout' }).waitFor({ timeout: 1000 });
      }
    });
    assert.ok(ms < 1000, `took ${ms} ms`);
    const by = await back.page.getByTestId('acked-by').first().textContent();
    assert.match(by, /FOH/);
  });

  await test('resolve from stage right clears it everywhere', async () => {
    const a = right.page.getByTestId('alert').filter({ hasText: 'Dropout' });
    await a.getByRole('button', { name: 'Resolve' }).click();
    await right.page.getByLabel('Resolution note').fill('Swapped pack');
    await right.page.getByRole('button', { name: 'Resolve', exact: true }).last().click();
    await foh.page.getByTestId('alert').filter({ hasText: 'Dropout' }).waitFor({ state: 'detached', timeout: 1000 });
    await back.page.getByTestId('alert').filter({ hasText: 'Dropout' }).waitFor({ state: 'detached', timeout: 1000 });
  });

  await test('song advance from FOH updates backstage', async () => {
    await foh.page.getByRole('button', { name: 'Next song', exact: true }).click();
    await back.page.getByTestId('current-song').filter({ hasText: 'Walk-in / Welcome' }).waitFor({ timeout: 1000 });
    await back.page.getByRole('button', { name: 'Next song', exact: true }).click();
    await foh.page.getByTestId('current-song').filter({ hasText: 'Song 1' }).waitFor({ timeout: 1000 });
  });

  await test('handoff marked done on one A2 screen shows on the other', async () => {
    const item = back.page.getByTestId('handoff').first();
    const text = (await item.textContent()).replace(/✓.*$/, '').trim().slice(0, 20);
    await item.click();
    const other = right.page.getByTestId('handoff').filter({ hasText: text }).first();
    await other.locator('.check', { hasText: '✔' }).waitFor({ timeout: 1000 });
  });

  await test('server restart: clients show offline, block edits, then resync', async () => {
    await stopServer(server);
    await back.page.getByTestId('connection').filter({ hasText: 'Offline' }).waitFor({ timeout: 3000 });
    assert.equal(await back.page.getByTestId('raise-alert').isDisabled(), true, 'raise alert disabled while offline');
    server = await startServer(db, port, false);
    for (const c of [back, foh, right]) {
      await c.page.getByTestId('connection').filter({ hasText: 'Live' }).waitFor({ timeout: 8000 });
    }
    // State survived and new changes still sync.
    await back.page.getByTestId('current-song').filter({ hasText: 'Song 1' }).waitFor({ timeout: 1000 });
    const r = await api(base, 'A2 Stage Right', 'POST', '/api/alerts', { severity: 'warning', title: 'After restart' });
    assert.equal(r.status, 201);
    await foh.page.getByTestId('alert').filter({ hasText: 'After restart' }).waitFor({ timeout: 1000 });
    await back.page.getByTestId('alert').filter({ hasText: 'After restart' }).waitFor({ timeout: 1000 });
    assert.equal(await back.page.getByTestId('alert').filter({ hasText: 'After restart' }).count(), 1, 'no duplicates');
  });

  await test('backstage touch targets are at least 48 px at 1080p', async () => {
    const small = await back.page.evaluate(() => {
      const els = document.querySelectorAll('main button.big, [data-testid=handoff], [data-testid=performer-card]');
      return [...els]
        .map((e) => ({ t: (e.textContent || '').trim().slice(0, 30), r: e.getBoundingClientRect() }))
        .filter(({ r }) => r.width > 0 && (r.height < 48 || r.width < 48))
        .map(({ t, r }) => `${t} ${Math.round(r.width)}x${Math.round(r.height)}`);
    });
    assert.deepEqual(small, []);
  });

  await test('viewer role cannot make changes', async () => {
    const v = await client(browser, base, 'Viewer', '#/backstage');
    assert.equal(await v.page.getByTestId('raise-alert').isDisabled(), true);
    const r = await api(base, 'Viewer', 'POST', '/api/alerts', { severity: 'info', title: 'nope' });
    assert.equal(r.status, 403);
    await v.ctx.close();
  });

  await test('swap to spare in 3 taps from an alert; FOH banner in < 1 s', async () => {
    const snap = await (await api(base, 'Admin', 'GET', '/api/snapshot')).json();
    const maya = snap.performers.find((p) => p.name === 'Maya').id;
    const r = await api(base, 'FOH', 'POST', '/api/alerts', {
      severity: 'critical',
      title: 'Vox 1 dropping out',
      performer_id: maya,
      show_id: snap.shows[0].id,
    });
    assert.equal(r.status, 201);
    const alert = back.page.getByTestId('alert').filter({ hasText: 'Vox 1 dropping out' });
    await alert.waitFor({ timeout: 1000 });
    // Tap 1: Swap on the alert. Tap 2: Swap to spare on the Vox 1 kit. Tap 3: the spare.
    await alert.getByTestId('alert-swap').click();
    await back.page.locator('.kit', { hasText: 'Vox 1' }).getByTestId('swap-kit').click();
    const banner = foh.page.getByTestId('swap-banner').filter({ hasText: 'Maya' });
    const ms = await timed(async () => {
      await back.page.getByTestId('swap-option').filter({ hasText: 'Spare HH A' }).click();
      await banner.waitFor({ timeout: 1000 });
    });
    assert.ok(ms < 1000, `took ${ms} ms`);
    const text = await banner.textContent();
    assert.match(text, /now on\s*Ch 9/);
    assert.match(text, /was Ch 2/);
    // FOH channel table follows, the alert is acknowledged, the failed pack is faulty.
    await foh.page.getByTestId('foh-row').filter({ hasText: 'Spare HH A' }).filter({ hasText: 'Maya' }).waitFor({ timeout: 1000 });
    await back.page.locator('[data-testid=alert][data-state=acknowledged]').filter({ hasText: 'Vox 1 dropping out' }).waitFor({ timeout: 1000 });
    const eq = await (await api(base, 'Admin', 'GET', '/api/equipment?kind=handheld_tx')).json();
    assert.equal(eq.find((e) => e.label === 'HH2').status, 'faulty');
    await foh.page.getByTestId('swap-banner').getByRole('button', { name: 'Got it' }).click();
    await foh.page.getByTestId('swap-banner').waitFor({ state: 'detached', timeout: 1000 });
  });

  await test('show mode: checklist, lock, automatic battery alerts', async () => {
    const admin = await client(browser, base, 'Admin', '#/admin', { width: 1440, height: 900 });
    await admin.page.getByRole('tab', { name: 'Show & kits' }).click();
    await admin.page.getByTestId('enter-show-mode').click();
    await admin.page.getByTestId('confirm-show-mode').click();
    await back.page.getByTestId('mode').filter({ hasText: 'SHOW MODE' }).waitFor({ timeout: 1000 });
    await admin.page.getByTestId('show-lock').waitFor({ timeout: 1000 });
    await admin.page.getByRole('tab', { name: /Assignments/ }).click();
    const firstCell = admin.page.locator('td select').first();
    assert.equal(await firstCell.isDisabled(), true, 'grid locked');
    // Server enforces it too.
    const snap = await (await api(base, 'Admin', 'GET', '/api/snapshot')).json();
    const show = snap.shows[0].id;
    const r = await api(base, 'Admin', 'PUT', `/api/shows/${show}/assignments/cell`, {
      performer_id: snap.performers[0].id,
      kind: 'mic',
      song_id: null,
      kit_ids: [],
    });
    assert.equal(r.status, 409);
    // Battery rule alerts for unchecked packs in use show up on FOH.
    const group = foh.page.getByTestId('battery-group');
    await group.waitFor({ timeout: 3000 });
    assert.match(await group.textContent(), /\d+ packs need a battery check/);
    // Grouped rows expand on tap.
    await group.click();
    await foh.page.getByTestId('alert').filter({ hasText: 'Battery not checked' }).first().waitFor({ timeout: 1000 });
    await admin.ctx.close();
  });

  await test('PIN change signs the screen out; signing in again on the PIN pad works', async () => {
    const snap = await (await api(base, 'Admin', 'GET', '/api/snapshot')).json();
    const sr = snap.users.find((u) => u.name === 'A2 Stage Right').id;
    const r = await api(base, 'Admin', 'PUT', `/api/users/${sr}/pin`, { pin: '4821' });
    assert.equal(r.status, 200);
    PINS['A2 Stage Right'] = '4821';
    // The old session no longer works: the screen goes to sign-in (socket recheck or next tap).
    await right.page.getByText('You were signed out').waitFor({ timeout: 15000 });
    await right.page.getByTestId('login-name').fill('A2 Stage Right');
    for (const k of '4821') await right.page.getByRole('button', { name: k, exact: true }).click();
    await right.page.getByRole('button', { name: 'OK', exact: true }).click();
    await right.page.getByTestId('raise-alert').waitFor({ timeout: 3000 });
    await right.page.getByTestId('raise-alert').click();
    await right.page.getByRole('button', { name: 'Crackle / noise' }).click();
    await right.page.getByRole('button', { name: 'Raise alert', exact: true }).last().click();
    await foh.page.getByTestId('alert').filter({ hasText: 'Crackle / noise' }).waitFor({ timeout: 1000 });
  });

  await test('helper joins with a 6-digit code typed on the pad', async () => {
    const h = await (await api(base, 'Admin', 'POST', '/api/users', { name: 'Jo', role: 'helper' })).json();
    const { code } = await (await api(base, 'Admin', 'POST', '/api/auth/join-codes', { user_id: h.id })).json();
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const page = await ctx.newPage();
    await page.goto(`${base}/#/backstage`);
    await page.getByRole('tab', { name: 'Have a code?' }).click();
    for (const k of code) await page.getByRole('button', { name: k, exact: true }).click();
    await page.getByRole('button', { name: 'OK', exact: true }).click();
    await page.getByTestId('connection').filter({ hasText: 'Live' }).waitFor({ timeout: 5000 });
    await ctx.close();
  });

  await test('signed-out browser sees sign-in, never show data', async () => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(`${base}/#/backstage`);
    await page.getByTestId('login-name').waitFor({ timeout: 3000 });
    const r = await page.request.get(`${base}/api/snapshot`);
    assert.equal(r.status(), 401);
    await ctx.close();
  });

  await test('post-show report lists the swap', async () => {
    const p = back.page;
    await p.goto(`${base}/#/report`);
    await p.getByText('post-show report').waitFor({ timeout: 2000 });
    await p.locator('td', { hasText: 'Spare HH A' }).first().waitFor({ timeout: 1000 });
    await p.locator('td', { hasText: 'HH2' }).first().waitFor({ timeout: 1000 });
    await p.goto(`${base}/#/backstage`);
  });

  await test('MIDI: Playback cue moves every screen in < 1 s; lock and follow', async () => {
    const k = await (await api(base, 'Admin', 'POST', '/api/midi/key/rotate', {})).json();
    const send = (value) =>
      fetch(`${base}/api/midi`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-rfops-bridge-key': k.key },
        body: JSON.stringify({ bridge: 'Playback Mac', port: 'RF Ops', messages: [[0xcf, value]] }),
      }).then((r) => r.json());
    // PC 3 on channel 16 = "Song 2" in the demo.
    const ms = await timed(async () => {
      const r = await send(3);
      assert.deepEqual(r.outcomes, ['→ Song 2']);
      for (const c of [back, foh]) await c.page.getByTestId('current-song').filter({ hasText: /^Song 2$/ }).waitFor({ timeout: 1000 });
    });
    assert.ok(ms < 1000, `took ${ms} ms`);
    await back.page.getByTestId('midi-status').filter({ hasText: 'Following Playback' }).waitFor({ timeout: 1000 });

    // Lock from the backstage screen: Playback cues are ignored, manual still works.
    await back.page.getByTestId('midi-toggle').click();
    await foh.page.getByTestId('midi-status').filter({ hasText: 'MIDI locked' }).waitFor({ timeout: 1000 });
    const locked = await send(5);
    assert.match(locked.outcomes[0], /^LOCKED/);
    await new Promise((r) => setTimeout(r, 300));
    assert.equal((await back.page.getByTestId('current-song').textContent()).trim(), 'Song 2');

    await back.page.getByTestId('midi-toggle').click();
    await back.page.getByTestId('midi-status').filter({ hasText: 'Following Playback' }).waitFor({ timeout: 1000 });
    await new Promise((r) => setTimeout(r, 550)); // past the duplicate window
    await send(5);
    await foh.page.getByTestId('current-song').filter({ hasText: 'Song 4' }).waitFor({ timeout: 1000 });
  });

  await test('MIDI: simulate from Admin, monitor shows it', async () => {
    const admin = await client(browser, base, 'Admin', '#/admin', { width: 1440, height: 900 });
    await admin.page.getByRole('tab', { name: 'MIDI' }).click();
    await admin.page.getByTestId('bridge-state').filter({ hasText: 'Online' }).waitFor({ timeout: 3000 });
    await admin.page.getByTestId('simulate-song').first().click(); // PC 1 = Walk-in / Welcome
    await back.page.getByTestId('current-song').filter({ hasText: 'Walk-in' }).waitFor({ timeout: 1000 });
    await admin.page.getByTestId('midi-monitor').getByText('simulated by Admin').first().waitFor({ timeout: 2000 });
    await admin.ctx.close();
  });

  await test('no page errors in any client', async () => {
    assert.deepEqual([...back.errors, ...foh.errors, ...right.errors], []);
  });

  await browser.close();
  await stopServer(server);
  fs.rmSync(dir, { recursive: true, force: true });
  const failed = results.filter((r) => !r.ok);
  console.log(`\n${results.length - failed.length}/${results.length} passed`);
  process.exit(failed.length ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
