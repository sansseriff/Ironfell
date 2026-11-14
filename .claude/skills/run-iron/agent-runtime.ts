import { chromium } from 'playwright-core';
import { once } from 'node:events';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { writeFile, mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { createAgentBridge } from '../../../scripts/agent-bridge/server';
import { codexRuntime } from '../../../scripts/agent-bridge/runtimes';

const url = process.env.URL ?? 'http://localhost:5175/Ironfell/?gfx=webgl2&showcase=0&demo';
if (!new URL(url).searchParams.has('demo')) throw new Error('Agent tests require an ephemeral ?demo URL');
const live = process.env.LIVE_AGENT === '1';
const runtimeMode = process.env.RUNTIME ?? 'worker';
const token = randomBytes(24).toString('hex');
const runtime = codexRuntime(live ? undefined : resolve('tests/fixtures/fake-agent-runtime.ts'));
const bridge = createAgentBridge({ token, origins: [new URL(url).origin], runtimes: { codex: runtime, claude: runtime }, timeoutMs: 20000 });
bridge.server.listen(0, '127.0.0.1');
await once(bridge.server, 'listening');
const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
const browser = await chromium.launch({
  executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'],
});
const deadline = setTimeout(() => { bridge.stop(); void browser.close(); }, 55000);
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const errors: string[] = [];
page.on('pageerror', error => errors.push(String(error)));
try {
  await page.goto(url);
  await page.evaluate(runtime => {
    const key = 'iron.settings.v1';
    localStorage.setItem(key, JSON.stringify({ ...JSON.parse(localStorage.getItem(key) ?? '{}'), runtime }));
  }, runtimeMode);
  await page.reload();
  await page.waitForFunction(() => !!(window as any).__iron, undefined, { timeout: 15000 });
  await page.waitForSelector('.loading', { state: 'detached', timeout: 15000 });
  await page.waitForFunction(async () => {
    try { return /<composition[^>]*name="main"/.test(await (window as any).__iron.view({ fidelity: 'skeleton' })); }
    catch { return false; }
  }, undefined, { timeout: 15000 });
  const result = await page.evaluate(async ({ endpoint, token }) => {
    const iron = (window as any).__iron;
    const before = await iron.save();
    iron.configureAgent({ provider: 'codex', endpoint, token });
    const setup = await iron.edit('<edit><composition id="$canvas" name="agent-sandbox" notation="free" dims="2d"/></edit>');
    if (!setup.ok) throw new Error(JSON.stringify(setup));
    const canvas = setup.aliases.$canvas;
    const created = await iron.ask(`TARGET ${canvas}. In this 2D composition create exactly one rect named agent-test-shape, x=100, y=120, w=80, h=60, fill.color=#ff2200. No other changes.`);
    if (!created.ok) throw new Error(`create failed: ${JSON.stringify(created)}`);
    const view = await iron.view({ fidelity: 'full', scope: canvas });
    const shape = view.match(/<rect id="([^"]+)"[^>]*name="agent-test-shape"/)?.[1];
    if (!shape) throw new Error(`shape not found: ${view}`);
    const edited = await iron.ask(`edit-test-shape. SHAPE ${shape}. Set only this rectangle's transform2d.x to 240 and fill.color to #22aa55. Preserve its name, y, width and height. No other changes.`);
    if (!edited.ok) throw new Error(`edit failed: ${JSON.stringify(edited)}`);
    const after = await iron.view({ fidelity: 'full', scope: shape });
    iron.undo();
    let undone = '';
    for (let attempt = 0; attempt < 30; attempt++) {
      undone = await iron.view({ fidelity: 'full', scope: shape });
      if (undone.includes('x="100"')) break;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    await iron.load(before);
    return { canvas, shape, created: { ok: created.ok, attempts: created.attempts, transport: created.transport, edit: created.edit }, edited: { ok: edited.ok, attempts: edited.attempts, transport: edited.transport, edit: edited.edit }, view, after, undone };
  }, { endpoint, token });
  assert.match(result.view, /x="100"/);
  assert.match(result.view, /(?:w="80"|w="80\.0")/);
  assert.match(result.after, /x="240"/);
  assert.match(result.after, /22aa55/i);
  assert.match(result.after, /y="120"/);
  assert.match(result.after, /w="80"/);
  assert.match(result.after, /h="60"/);
  assert.match(result.undone, /x="100"/);
  assert.deepEqual(errors, []);
  await mkdir('.agents/agent-runtime', { recursive: true });
  await writeFile(`.agents/agent-runtime/${live ? 'live' : 'fixture'}-${runtimeMode}.json`, JSON.stringify(result, null, 2));
  console.log(`PASS ${live ? 'LIVE CODEX' : 'FIXTURE CODEX'} ${runtimeMode}: create, edit, preserve dimensions, undo; no page errors`);
} finally {
  clearTimeout(deadline);
  bridge.stop();
  await browser.close();
}
