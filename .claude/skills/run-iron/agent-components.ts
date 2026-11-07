// The agent builds a component pictured in two 2D compositions, through the
// real engine and a scripted runtime (tests/fixtures/fake-agent-runtime.ts):
// <make-component> of two shapes, <place> of it in one composition and
// <place entity> in a second, a std:laser with an argument, all one history
// entry; then a <look expand> at one picture before an edit. LIVE_AGENT=1
// asks the signed-in Codex instead (consumes model usage).
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
const bridge = createAgentBridge({ token, origins: [new URL(url).origin], runtimes: { codex: runtime, claude: runtime }, timeoutMs: 40000 });
bridge.server.listen(0, '127.0.0.1');
await once(bridge.server, 'listening');
const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
const browser = await chromium.launch({
  executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'],
});
const deadline = setTimeout(() => { bridge.stop(); void browser.close(); }, 85000);
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
    const setup = await iron.edit(`<edit>
      <create id="$bench" type="composition" name="agent-bench" notation="free" dims="2d"/>
      <create id="$overview" type="composition" name="agent-overview" notation="free" dims="2d"/>
      <create id="$body" type="rect" parent="$bench" name="body" x="100" y="50" w="60" h="40" color="#3366cc"/>
      <create id="$dot" type="ellipse" parent="$bench" name="dot" x="130" y="70" rx="8" ry="8" color="#ffcc00"/>
    </edit>`);
    if (!setup.ok) throw new Error(JSON.stringify(setup));
    const { $bench: bench, $overview: overview, $body: body, $dot: dot } = setup.aliases;
    const undoDepth = () => iron.history?.()?.past?.length;
    const placed = await iron.ask(`component-test-place. SHAPES ${body} ${dot} IN ${bench} ${overview}. Make a component named pump of the rect and the ellipse in agent-bench, place an instance P1 of it at 300, 80 in agent-bench and draw the same entity at 40, 40 in agent-overview, and add a standard laser L1 at 40, 200 in agent-bench with power 5 mW. Use the component verbs.`);
    if (!placed.ok) throw new Error(`place failed: ${JSON.stringify(placed)}`);
    const summary: string = placed.outcome?.summary ?? '';
    const attr = (tag: string, n: number, a: string) => [...summary.matchAll(new RegExp(`<${tag} [^>]*${a}="([^"]+)"`, 'g'))][n]?.[1];
    const entity = attr('placed', 0, 'entity'), use1 = attr('placed', 0, 'use'), use2 = attr('placed', 1, 'use');
    const model = await iron.view({ fidelity: 'full', relation: 'model' });
    const benchView = await iron.view({ fidelity: 'full', scope: bench });
    const overviewView = await iron.view({ fidelity: 'full', scope: overview });
    const looked = await iron.ask(`component-test-look. ENTITY ${entity} USE ${use2}. Look at the picture of P1 in agent-overview, then rename P1 to "P1 seen".`);
    const renamed = await iron.view({ fidelity: 'full', relation: 'model' });
    // One undo takes back the rename, a second everything the place made.
    iron.undo();
    await new Promise(resolve => setTimeout(resolve, 100));
    iron.undo();
    let undone = '';
    for (let attempt = 0; attempt < 30; attempt++) {
      undone = await iron.view({ fidelity: 'full', scope: bench });
      if (!undone.includes(`id="${use1}"`)) break;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    await iron.load(before);
    return {
      bench, overview, body, dot, entity, use1, use2, summary, undoDepth: undoDepth(),
      placed: { ok: placed.ok, attempts: placed.attempts, edit: placed.edit },
      looked: { ok: looked.ok, attempts: looked.attempts, edit: looked.edit, error: looked.error, note: looked.note },
      model, benchView, overviewView, renamed, undone,
    };
  }, { endpoint, token });
  assert.ok(result.entity && result.use1 && result.use2, `summary names the entity and both pictures: ${result.summary}`);
  assert.match(result.summary, /<made definition="#\w+" main="#\w+"\/>/);
  assert.equal((result.summary.match(/<placed /g) ?? []).length, 3, 'P1 twice and the laser');
  // The entity is pictured in both compositions, each a use figuring it.
  const figures = result.model.match(new RegExp(`<entity id="${result.entity}"[\\s\\S]*?</entity>`))?.[0] ?? '';
  assert.match(figures, new RegExp(`<figure[^>]*${result.use1}[^>]*`), figures);
  assert.match(figures, new RegExp(`<figure[^>]*${result.use2}[^>]*`), figures);
  assert.match(result.benchView, new RegExp(`<use id="${result.use1}"[^>]*x="300"`));
  assert.match(result.overviewView, new RegExp(`<use id="${result.use2}"[^>]*x="40"`));
  assert.match(result.model, /name="L1"[^>]*power="5 mW"|power="5 mW"[^>]*name="L1"/);
  // The look came back with the derived picture, and did not use the repair attempt.
  assert.equal(result.looked.ok, true, JSON.stringify(result.looked));
  assert.equal(result.looked.attempts, 1);
  assert.match(result.renamed, /name="P1 seen"/);
  assert.doesNotMatch(result.undone, new RegExp(`id="${result.use1}"`));
  assert.match(result.undone, new RegExp(`id="${result.body}"`), 'undo puts the shapes back');
  assert.deepEqual(errors, []);
  await mkdir('.agents/agent-runtime', { recursive: true });
  await writeFile(`.agents/agent-runtime/components-${live ? 'live' : 'fixture'}-${runtimeMode}.json`, JSON.stringify(result, null, 2));
  console.log(`PASS ${live ? 'LIVE' : 'FIXTURE'} COMPONENTS ${runtimeMode}: make, place in two compositions, std:laser with an argument, look expand, one undo each; no page errors`);
} finally {
  clearTimeout(deadline);
  bridge.stop();
  await browser.close();
}
