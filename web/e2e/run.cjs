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
    env: { ...process.env, RFOPS_DB: `sqlite://${db}`, RFOPS_ADDR: `127.0.0.1:${port}`, RFOPS_WEB: WEB, RUST_LOG: 'warn' },
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

async function client(browser, base, operator, hash, viewport = { width: 1920, height: 1080 }) {
  const ctx = await browser.newContext({ viewport });
  await ctx.addInitScript((op) => localStorage.setItem('rfops.operator', op), operator);
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
    const r = await fetch(`${base}/api/alerts`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-rfops-actor': 'A2 Stage Right' },
      body: JSON.stringify({ severity: 'warning', title: 'After restart' }),
    });
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
    const r = await fetch(`${base}/api/alerts`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-rfops-actor': 'Viewer' },
      body: JSON.stringify({ severity: 'info', title: 'nope' }),
    });
    assert.equal(r.status, 403);
    await v.ctx.close();
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
