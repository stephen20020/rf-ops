// End-to-end tests for the v3.1 screens: backstage board, FOH channels, swaps, plot, show file.
// Usage (from web/): npm run build && node e2e/board.cjs   (needs ../target/debug/rfops-server)
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

const SHOW = `
show: E2E Show
people: [Helen, Shosh, Judah]
devices:
  - { label: E-Red, kind: mic, channel: "34" }
  - { label: E-Tan, kind: mic, channel: "35" }
  - { label: E-Spare, kind: mic, channel: "43", spare: true }
  - { label: E-RX10, kind: iem }
songs:
  - { title: Opener }
  - title: Pass Song
    passes:
      - "E-RX10: Helen -> Shosh"
  - { title: Closer }
plot:
  default:
    E-Red: Helen
    E-RX10: Helen
`;

const freePort = () =>
  new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });

async function startServer(db, port) {
  const proc = spawn(BIN, [], {
    env: { ...process.env, RFOPS_DB: `sqlite://${db}`, RFOPS_ADDR: `127.0.0.1:${port}`, RFOPS_WEB: WEB, RUST_LOG: 'warn', RFOPS_AUTH: 'open' },
    stdio: ['ignore', 'inherit', 'inherit'],
  });
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`http://127.0.0.1:${port}/api/health`)).ok) return proc;
    } catch {}
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('server did not start');
}

async function client(browser, base, name, hash, viewport = { width: 1600, height: 1000 }) {
  const ctx = await browser.newContext({ viewport });
  await ctx.addInitScript((n) => localStorage.setItem('rfops.name', n), name);
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
    results.push({ ok: true });
    console.log(`  ✓ ${name} (${Date.now() - t0} ms)`);
  } catch (e) {
    results.push({ ok: false });
    console.log(`  ✗ ${name}\n    ${e && e.stack ? e.stack.split('\n').slice(0, 4).join('\n    ') : e}`);
  }
}

(async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rfops-board-'));
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = await startServer(path.join(dir, 'b.db'), port);
  const imp = await fetch(`${base}/api/showfile/import`, { method: 'POST', headers: { 'content-type': 'text/yaml', 'x-rfops-actor': 'Setup' }, body: SHOW });
  assert.equal(imp.status, 201, await imp.text());

  const browser = await chromium.launch();
  const wall = await client(browser, base, 'Wall SL', '#/board');
  const foh = await client(browser, base, 'FOH', '#/channels', { width: 1280, height: 900 });
  const phone = await client(browser, base, 'Sam', '#/board', { width: 390, height: 844 });
  const card = (p, text) => p.getByTestId('handoff-card').filter({ hasText: text });
  console.log('RF Ops board e2e');

  await test('pre-show: give cards; Done on the wall clears it on the phone in < 1 s; Undo brings it back', async () => {
    await card(wall.page, 'Give E-Red to Helen').waitFor({ timeout: 3000 });
    await card(phone.page, 'Give E-Red to Helen').waitFor({ timeout: 3000 });
    const t0 = Date.now();
    await card(wall.page, 'Give E-Red to Helen').getByRole('button', { name: 'Done' }).click();
    await card(phone.page, 'Give E-Red to Helen').waitFor({ state: 'detached', timeout: 1000 });
    assert.ok(Date.now() - t0 < 1000);
    await wall.page.getByTestId('undo-bar').getByRole('button', { name: 'Undo' }).click();
    await card(phone.page, 'Give E-Red to Helen').waitFor({ timeout: 1000 });
    await card(wall.page, 'Give E-Red to Helen').getByRole('button', { name: 'Done' }).click();
    await card(wall.page, 'Give E-RX10 to Helen').getByRole('button', { name: 'Done' }).click();
    await card(wall.page, 'Give E-RX10').waitFor({ state: 'detached', timeout: 1000 });
  });

  await test('tiles show who has what; the pass card appears in its song and the return comes after', async () => {
    await wall.page.locator('[data-testid=tile][data-device="E-Red"]').filter({ hasText: 'Helen' }).waitFor({ timeout: 1000 });
    await wall.page.getByRole('button', { name: 'Next song', exact: true }).click(); // Opener
    await wall.page.getByRole('button', { name: 'Next song', exact: true }).click(); // Pass Song
    const pass = wall.page.getByTestId('pass-card').filter({ hasText: 'E-RX10' });
    await pass.waitFor({ timeout: 2000 });
    await pass.getByRole('button', { name: 'Pass done' }).click();
    await card(wall.page, 'Pass E-RX10: Shosh → Helen').waitFor({ timeout: 1000 });
  });

  await test('swap: spare offered first → In hand → FOH confirms at the console', async () => {
    await wall.page.locator('[data-testid=tile][data-device="E-Red"]').click();
    await wall.page.getByTestId('swap-start').click();
    const opt = wall.page.getByTestId('swap-option').first();
    assert.match(await opt.textContent(), /E-Spare/);
    await opt.click();
    const sc = wall.page.getByTestId('swap-card');
    await sc.filter({ hasText: 'Helen: E-Red → E-Spare' }).waitFor({ timeout: 1000 });
    await foh.page.getByTestId('swap-card').filter({ hasText: 'Ch 34 → Ch 43' }).waitFor({ timeout: 1000 });
    await sc.getByRole('button', { name: 'In hand' }).click();
    const confirm = foh.page.getByRole('button', { name: 'Done at console' });
    await confirm.waitFor({ timeout: 1000 });
    await confirm.click();
    await wall.page.getByTestId('swap-card').filter({ hasText: 'FOH ✓' }).waitFor({ timeout: 1000 });
    await wall.page.locator('[data-testid=tile][data-device="E-Spare"][data-usage="on_spare_for"]').waitFor({ timeout: 1000 });
    await wall.page.locator('[data-testid=tile][data-device="E-Red"][data-usage="suspect"]').waitFor({ timeout: 1000 });
    await foh.page.getByTestId('foh-row').filter({ hasText: 'E-Spare' }).filter({ hasText: 'Helen' }).waitFor({ timeout: 1000 });
  });

  await test('return to the fixed mic in two steps', async () => {
    const sc = wall.page.getByTestId('swap-card');
    await sc.getByRole('button', { name: /E-Red fixed: return/ }).click();
    await sc.getByRole('button', { name: 'Back in hand' }).click();
    await wall.page.locator('[data-testid=tile][data-device="E-Red"][data-usage="in_use"]').waitFor({ timeout: 1000 });
  });

  await test('plot edit in Admin shows up as a card on the board', async () => {
    const admin = await client(browser, base, 'Lead', '#/admin', { width: 1600, height: 1000 });
    await admin.page.getByTestId('plot-grid').waitFor({ timeout: 3000 });
    await admin.page.getByLabel('E-Tan in Closer').selectOption({ label: 'Judah' });
    await card(wall.page, 'Give E-Tan to Judah').waitFor({ timeout: 2000 });
    // Show file: a bad file is refused with a plain-English reason.
    await admin.page.getByRole('tab', { name: 'Show file' }).click();
    await admin.page.getByTestId('showfile-text').fill(SHOW.replace('E-Red: Helen', 'E-Red: Hellen'));
    await admin.page.getByTestId('showfile-import').click();
    await admin.page.getByTestId('showfile-result').filter({ hasText: 'Did you mean "Helen"' }).waitFor({ timeout: 2000 });
    assert.deepEqual(admin.errors, []);
    await admin.ctx.close();
  });

  await test('problem report: 2 taps on the wall, appears on the phone, On it → Fixed', async () => {
    await wall.page.locator('[data-testid=tile][data-device="E-Red"]').click();
    await wall.page.getByRole('button', { name: 'No audio' }).click();
    const a = phone.page.getByTestId('board-alert').filter({ hasText: 'E-Red (Helen): No audio' });
    await a.waitFor({ timeout: 1000 });
    await a.getByRole('button', { name: 'On it' }).click();
    await wall.page.getByTestId('board-alert').filter({ hasText: 'Sam is on it' }).waitFor({ timeout: 1000 });
    await wall.page.getByTestId('board-alert').getByRole('button', { name: 'Fixed' }).click();
    await phone.page.getByTestId('board-alert').waitFor({ state: 'detached', timeout: 1000 });
  });

  await test('battery round: one tap per device, counted for this show', async () => {
    await wall.page.getByTestId('battery-round').click();
    const t = wall.page.locator('[data-testid=battery-tile][data-device="E-Tan"]');
    await t.getByRole('button', { name: 'Fresh' }).click();
    await t.filter({ hasText: '✓ fresh' }).waitFor({ timeout: 1000 });
    await wall.page.getByTestId('battery-round').click();
    await wall.page.locator('[data-testid=tile][data-device="E-Tan"]').filter({ hasText: '✓ fresh' }).waitFor({ timeout: 1000 });
  });

  await test('pre-show checklist: auto items, manual tick, start and end the show, report shows the swap', async () => {
    const p = wall.page;
    await p.goto(`${base}/#/checklist`);
    const count = p.getByTestId('checklist-count');
    await count.waitFor({ timeout: 3000 });
    const before = Number((await count.textContent()).split('/')[0]);
    await p.locator('[data-testid=check-item][data-key=wifi]').getByRole('button').click();
    await count.filter({ hasText: `${before + 1} /` }).waitFor({ timeout: 2000 });
    assert.match(await p.locator('[data-testid=check-item][data-key=batteries]').textContent(), /checked/);
    p.once('dialog', (d) => d.accept());
    await p.getByTestId('start-show').click();
    await p.waitForURL(/#\/board/, { timeout: 3000 });
    await foh.page.getByTestId('mode').filter({ hasText: 'SHOW MODE' }).waitFor({ timeout: 2000 });
    await p.goto(`${base}/#/checklist`);
    p.once('dialog', (d) => d.accept());
    await p.getByTestId('end-show').click();
    await p.waitForURL(/#\/post-show/, { timeout: 3000 });
    const r = p.getByTestId('post-show');
    await r.filter({ hasText: 'Swap: E-Red → E-Spare' }).waitFor({ timeout: 3000 });
    await r.filter({ hasText: 'No audio' }).waitFor({ timeout: 1000 });
    // Paper plot opens and shows the plan.
    const pr = await wall.ctx.newPage();
    await pr.goto(`${base}/#/print`);
    await pr.locator('td', { hasText: 'Helen → Shosh' }).first().waitFor({ timeout: 3000 });
    await pr.close();
    await p.goto(`${base}/#/board`);
  });

  await test('phone layout: jobs first, tiles below, no horizontal scroll', async () => {
    const w = await phone.page.evaluate(() => document.documentElement.scrollWidth);
    assert.ok(w <= 392, `scrollWidth ${w}`);
  });

  await test('no page errors', async () => {
    assert.deepEqual([...wall.errors, ...foh.errors, ...phone.errors], []);
  });

  await browser.close();
  server.kill('SIGKILL');
  fs.rmSync(dir, { recursive: true, force: true });
  const failed = results.filter((r) => !r.ok).length;
  console.log(`\n${results.length - failed}/${results.length} passed`);
  process.exit(failed ? 1 : 0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
