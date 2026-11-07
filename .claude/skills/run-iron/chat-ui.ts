import { chromium } from 'playwright-core';
import { once } from 'node:events';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { mkdir, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { agentLog } from '../../../scripts/agent-bridge/log';
import assert from 'node:assert/strict';
import { createAgentBridge } from '../../../scripts/agent-bridge/server';
import { codexRuntime } from '../../../scripts/agent-bridge/runtimes';

const url = process.env.URL ?? 'http://127.0.0.1:5175/Ironfell/?gfx=webgl2&demo';
if (!new URL(url).searchParams.has('demo')) throw new Error('Chat tests require an ephemeral ?demo URL');
const runtimeMode = process.env.RUNTIME ?? 'worker';
const live = process.env.LIVE_AGENT === '1';
const token = randomBytes(24).toString('hex');
const runtime = codexRuntime(live ? undefined : resolve('tests/fixtures/fake-agent-runtime.ts'));
// The agent log (scripts/agent-bridge/log.ts), checked at the end against the page's turns.
const logDir = await mkdtemp(resolve(tmpdir(), 'iron-chat-log-'));
const log = agentLog(logDir);
const bridge = createAgentBridge({ token, origins: [new URL(url).origin], runtimes: { codex: runtime, claude: runtime }, timeoutMs: live ? 20000 : 5000, log });
bridge.server.listen(0, '127.0.0.1');
await once(bridge.server, 'listening');
const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
const deadline = setTimeout(() => { bridge.stop(); void browser.close(); }, 55000);
const page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
page.setDefaultTimeout(8000);
const errors: string[] = [];
page.on('pageerror', error => errors.push(String(error)));
page.on('requestfailed', request => console.error('Request failed:', request.url(), request.failure()?.errorText));

/** Every Model/Lesson turn the page ran through the bridge has its calls and its outcome in the agent log. */
async function checkAgentLog() {
  // The page posts a turn's outcome without waiting; let it land.
  await new Promise(resolve => setTimeout(resolve, 300));
  await log.flushed();
  const records = (await Promise.all((await readdir(logDir)).map(file => readFile(resolve(logDir, file), 'utf8'))))
    .join('').trim().split('\n').map(line => JSON.parse(line));
  const ids = await page.locator('.chat-turn').evaluateAll(turns => turns.filter(t => t.getAttribute('data-status') !== 'running').map(t => t.getAttribute('data-turn-id')));
  const calls = new Set(records.filter(r => r.kind === 'call').map(r => r.turn));
  const outcomes = new Map(records.filter(r => r.kind === 'turn').map(r => [r.turn, r]));
  assert.ok(records.some(r => r.kind === 'system'), 'the log holds the system prompt');
  for (const id of ids) {
    assert.ok(calls.has(id), `turn ${id} has its model calls in the agent log`);
    assert.ok(outcomes.has(id), `turn ${id} has its outcome in the agent log`);
  }
  return ids.length;
}

try {
  await page.goto(url);
  await page.evaluate(mode => localStorage.setItem('iron.settings.v1', JSON.stringify({ runtime: mode })), runtimeMode);
  await page.reload();
  await page.waitForFunction(async () => {
    try { return /name="main"/.test(await (window as any).__iron.view({ fidelity: 'skeleton' })); } catch { return false; }
  }, undefined, { timeout: 15000 });
  await page.getByRole('tab', { name: 'Model', exact: true }).waitFor();
  await page.waitForSelector('.loading', { state: 'detached', timeout: 15000 });
  await page.waitForFunction(async () => {
    try { return !!(await (window as any).__iron.call('schema')).types; } catch { return false; }
  }, undefined, { timeout: 15000 });
  await page.getByRole('tab', { name: 'Model', exact: true }).click();
  await page.evaluate(({ endpoint, token }) => (window as any).__iron.configureAgent({ provider: 'codex', endpoint, token }), { endpoint, token });
  const prompt = page.getByRole('textbox', { name: 'Agent instruction' });
  if (live) {
    const before = await page.evaluate(() => (window as any).__iron.save());
    await prompt.fill('Do not edit the document. Explain in a short Markdown heading and two bullets what a 2D composition is. Then show the existing main composition by putting ::view{show=main} on its own paragraph, separated by blank lines. Reply with prose and that visual reference only, no edit block.');
    await page.getByRole('button', { name: 'Send message', exact: true }).click();
    await page.locator('.chat-turn[data-status="completed"]').waitFor({ timeout: 25000 });
    assert.equal(await page.evaluate(() => (window as any).__iron.save()), before);
    assert.ok(await page.locator('.chat-markdown').count() > 0);
    await page.waitForFunction(() => (document.querySelector('[data-chat-visual] img') as HTMLImageElement)?.naturalWidth > 0, undefined, { timeout: 10000 });
    const canvas = await page.evaluate(async () => (await (window as any).__iron.view({ fidelity: 'skeleton' })).match(/<composition id="([^"]+)" name="main"/)?.[1]);
    assert.ok(canvas);
    await prompt.fill(`In composition ${canvas}, create exactly one rect named chat-live-shape at x=20, y=20, w=30, h=40, fill.color=#2266cc. Make no other changes. Put the executable <edit> outside Markdown code fences.`);
    await page.getByRole('button', { name: 'Send message', exact: true }).click();
    await page.waitForFunction(() => document.querySelectorAll('.chat-turn[data-status="completed"]').length === 2, undefined, { timeout: 25000 });
    const after = await page.evaluate(async canvas => (window as any).__iron.view({ fidelity: 'full', scope: canvas }), canvas);
    assert.match(after, /name="chat-live-shape"/);
    assert.ok(await page.locator('.edit-trigger').count() > 0);
    assert.deepEqual(errors, []);
    await mkdir('.agents/chat', { recursive: true });
    await page.screenshot({ path: `.agents/chat/live-${runtimeMode}.png` });
    const logged = await checkAgentLog();
    console.log(`PASS LIVE CHAT ${runtimeMode}: real Codex prose/visual without mutation, then validated shape creation, ${logged} turns in the agent log; no page errors`);
  } else {
  await prompt.fill('chat-test-prose');
  await prompt.dispatchEvent('keydown', { key: 'Enter', metaKey: true, isComposing: true });
  assert.equal(await page.locator('.chat-turn').count(), 0);
  await prompt.press('Meta+Enter');
  await page.locator('.chat-turn[data-status="completed"]').waitFor();
  await page.locator('[data-chat-visual] img').waitFor();
  assert.equal(await page.locator('.chat-markdown h2').textContent(), 'A shared conversation');
  assert.equal(await page.locator('.chat-markdown table').count(), 1);
  assert.equal(await page.locator('.chat-markdown pre code').count(), 1);
  assert.equal(await page.locator('a[href^="javascript:"]').count(), 0);
  assert.equal(await page.evaluate(() => (window as any).chatInjected), undefined);
  await page.waitForFunction(() => (document.querySelector('[data-chat-visual] img') as HTMLImageElement)?.naturalWidth > 0);
  await mkdir('.agents/chat', { recursive: true });
  await page.screenshot({ path: `.agents/chat/model-${runtimeMode}.png` });
  await prompt.fill('chat-test-follow-up');
  await prompt.press('Meta+Enter');
  await page.waitForFunction(() => document.querySelectorAll('.chat-turn[data-status="completed"]').length === 2);
  assert.match(await page.locator('.chat-turn').last().innerText(), /previous reply described a rectangle/);
  await prompt.fill('chat-test-slow');
  await prompt.press('Meta+Enter');
  await page.locator('.chat-turn[data-status="running"]').waitFor();
  await page.locator('.transcript').evaluate(element => { element.scrollTop = 0; element.dispatchEvent(new Event('scroll')); });
  await page.waitForFunction(() => document.querySelectorAll('.chat-turn[data-status="completed"]').length === 3);
  await page.getByRole('button', { name: 'Latest', exact: true }).waitFor();
  assert.equal(await page.locator('.transcript').evaluate(element => element.scrollTop), 0);
  await page.getByRole('button', { name: 'Latest', exact: true }).click();
  await page.waitForFunction(() => { const element = document.querySelector('.transcript')!; return element.scrollHeight - element.scrollTop - element.clientHeight < 64; });
  await prompt.fill('Model draft stays here');
  await page.getByRole('tab', { name: 'Lesson', exact: true }).click();
  await page.waitForFunction(() => ((window as any).__narrative?.get('lesson')?.status().blocks ?? 0) > 2);
  const lessonPrompt = page.getByRole('textbox', { name: 'Agent instruction' });
  await lessonPrompt.fill('Lesson draft survives modes');
  const cursor = await page.evaluate(() => (window as any).__narrative.get('lesson').editor().view.state.selection.from);
  for (const mode of ['hybrid', 'exploration', 'authoring', 'hybrid']) {
    await page.locator(`[data-mode-button="${mode}"]`).click();
    assert.equal(await lessonPrompt.inputValue(), 'Lesson draft survives modes');
    assert.equal(await page.evaluate(() => (window as any).__narrative.get('lesson').editor().view.state.selection.from), cursor);
  }
  await page.locator('[data-mode-button="exploration"]').click();
  assert.equal(await page.locator('.narrative .outline').count(), 1);
  assert.equal(await page.locator('.narrative .scroll').isVisible(), false);
  const before = await page.evaluate(() => (window as any).__iron.save());
  await lessonPrompt.fill('chat-test-lesson');
  await lessonPrompt.press('Meta+Enter');
  await page.locator('.narrative .chat-turn[data-status="completed"]').waitFor();
  assert.equal(await page.evaluate(() => (window as any).__iron.save()), before);
  await page.locator('[data-mode-button="hybrid"]').click();
  await page.screenshot({ path: `.agents/chat/hybrid-${runtimeMode}.png` });
  await page.evaluate(() => (window as any).__narrative.get('lesson').askWith('<edit><write narrative="lesson" after="end">Generated shared-chat review paragraph.</write></edit>', 'Add a paragraph'));
  assert.ok(await page.locator('.nb-changed').count() > 0);
  await page.getByRole('button', { name: 'Revert', exact: true }).click();
  await page.waitForFunction(async () => !(await (window as any).__iron.call('narrative_read', ['lesson'])).blocks.some((block: any) => block.text === 'Generated shared-chat review paragraph.'));
  await page.evaluate(() => (window as any).__narrative.get('lesson').askWith('<edit><write narrative="lesson" after="end">Another model paragraph.</write></edit>', 'Add another paragraph'));
  await page.evaluate(async () => {
    const panel = (window as any).__narrative.get('lesson');
    const editor = panel.editor();
    editor.view.dispatch(editor.view.state.tr.insertText(' Newer human prose.', 1));
    await panel.flush();
    await panel.revert(panel.turns().length - 1);
  });
  assert.match(await page.locator('.narrative').innerText(), /would overwrite newer work/);
  await page.locator('[data-mode-button="authoring"]').click();
  await page.screenshot({ path: `.agents/chat/authoring-${runtimeMode}.png` });
  await page.locator('[data-mode-button="exploration"]').click();
  await page.screenshot({ path: `.agents/chat/exploration-${runtimeMode}.png` });
  await page.getByRole('tab', { name: 'Model', exact: true }).click();
  assert.equal(await prompt.inputValue(), 'Model draft stays here');
  assert.equal(await page.locator('.chat-turn').count(), 3);
  const beforeFailure = await page.evaluate(() => (window as any).__iron.save());
  await prompt.fill('chat-test-failure');
  await prompt.press('Meta+Enter');
  await page.locator('.chat-turn[data-status="failed"]').waitFor();
  assert.equal(await prompt.inputValue(), 'chat-test-failure');
  await prompt.press('Meta+Enter');
  await page.locator('.chat-turn[data-status="running"]').waitFor();
  await prompt.fill('A newer unsent draft');
  await page.waitForFunction(() => document.querySelectorAll('.chat-turn[data-status="failed"]').length === 2);
  assert.equal(await prompt.inputValue(), 'A newer unsent draft');
  assert.equal(await page.evaluate(() => (window as any).__iron.save()), beforeFailure);
  // Live progress (plans/specs/agent-streaming.md): the work log, reasoning,
  // streamed prose and the edit's verbs as they close, then Stop.
  await prompt.fill('chat-test-stream');
  await prompt.press('Meta+Enter');
  const running = page.locator('.chat-turn[data-status="running"]');
  await running.locator('[data-chat-progress]', { hasText: 'Thinking' }).waitFor();
  await running.locator('[data-chat-progress]', { hasText: 'Sizing the pump' }).waitFor();
  await running.locator('[data-edit-forming] li', { hasText: 'Create rect "inlet"' }).waitFor({ timeout: 15000 });
  assert.match(await running.locator('.chat-markdown').first().innerText(), /Here is a pump/);
  assert.match(await running.locator('[data-chat-progress]').innerText(), /Writing the edit/);
  assert.match(await running.locator('[data-chat-progress]').innerText(), /~\d+ tokens/);
  await page.screenshot({ path: `.agents/chat/streaming-${runtimeMode}.png` });
  await page.waitForFunction(() => document.querySelectorAll('.chat-turn[data-status="completed"]').length === 4, undefined, { timeout: 15000 });
  const streamedTurn = page.locator('.chat-turn').last();
  assert.match(await streamedTurn.locator('[data-chat-progress]').innerText(), /Worked for/);
  // The runtime's exact count replaces the estimate once the call ends.
  assert.match(await streamedTurn.locator('[data-chat-progress]').innerText(), /42 tokens \(30 thinking\)/);
  assert.match(await page.evaluate(() => (window as any).__iron.view({ fidelity: 'skeleton' })), /pump body/);
  await page.screenshot({ path: `.agents/chat/streamed-${runtimeMode}.png` });
  const beforeStop = await page.evaluate(() => (window as any).__iron.save());
  await prompt.fill('chat-test-stop');
  await prompt.press('Meta+Enter');
  await running.locator('[data-chat-progress]', { hasText: 'two ports' }).waitFor();
  await page.getByRole('button', { name: 'Stop the agent', exact: true }).click();
  await page.locator('.chat-turn[data-status="stopped"]').waitFor();
  assert.equal(await prompt.inputValue(), 'chat-test-stop');
  assert.equal(await page.evaluate(() => (window as any).__iron.save()), beforeStop);
  assert.deepEqual(errors, []);
  const logged = await checkAgentLog();
  console.log(`PASS CHAT ${runtimeMode}: shared Model/Lesson, Markdown/visuals, safe content, history, scroll follow, IME, drafts/modes/cursor, prose-only, review/revert/conflict, failure recovery, live progress and Stop, ${logged} turns in the agent log; no page errors`);
  }
} catch (error) {
  await mkdir('.agents/chat', { recursive: true });
  await page.screenshot({ path: `.agents/chat/failure-${runtimeMode}.png` }).catch(() => {});
  console.error({ errors, text: await page.locator('body').innerText().catch(() => '') });
  throw error;
} finally {
  clearTimeout(deadline);
  bridge.stop();
  await browser.close();
  // KEEP_AGENT_LOG=1 keeps the run's agent log to read.
  if (process.env.KEEP_AGENT_LOG === '1') console.log(`Agent log: ${logDir}`);
  else await rm(logDir, { recursive: true, force: true });
}
