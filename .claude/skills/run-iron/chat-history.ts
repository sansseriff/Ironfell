// Chat history (src-ui/chat/sessions.svelte.ts): New chat, the past-chats
// menu, delete, a separate history per panel, and chats saved with the scope
// (`chats.json`) and restored after a reload. Needs a saved scope, so it opens
// the app without ?demo, in a fresh browser profile (its own OPFS), creating a demo scope (&new=demo).
import { chromium } from 'playwright-core';
import { once } from 'node:events';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';
import { createAgentBridge } from '../../../scripts/agent-bridge/server';
import { codexRuntime } from '../../../scripts/agent-bridge/runtimes';

const url = process.env.URL ?? 'http://127.0.0.1:5175/Ironfell/?gfx=webgl2&new=demo';
if (new URL(url).searchParams.has('demo')) throw new Error('Chat history needs a saved scope: use a URL without ?demo');
const token = randomBytes(24).toString('hex');
const bridge = createAgentBridge({ token, origins: [new URL(url).origin], runtimes: { codex: codexRuntime(resolve('tests/fixtures/fake-agent-runtime.ts')), claude: codexRuntime(resolve('tests/fixtures/fake-agent-runtime.ts')) }, timeoutMs: 5000 });
bridge.server.listen(0, '127.0.0.1');
await once(bridge.server, 'listening');
const endpoint = `http://127.0.0.1:${(bridge.server.address() as { port: number }).port}`;
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
const deadline = setTimeout(() => { bridge.stop(); void browser.close(); }, 55000);
const page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
page.setDefaultTimeout(8000);
const errors: string[] = [];
page.on('pageerror', error => errors.push(String(error)));

async function ready() {
  // ?new=demo makes the scope and the address then names it (scope=…).
  // Ready once that scope is open and holds the demo; polled from here, as
  // an async predicate in waitForFunction can pass before it settles.
  const until = Date.now() + 20000;
  for (;;) {
    const open = await page.evaluate(async () => {
      try {
        const id = new URLSearchParams(location.search).get('scope');
        if (!id || (window as any).__iron?.scope?.().current?.id !== id) return false;
        return /name="main"/.test(await (window as any).__iron.view({ fidelity: 'skeleton' }));
      } catch { return false; }
    }).catch(() => false);
    if (open) break;
    if (Date.now() > until) throw new Error('the scope did not open');
    await page.waitForTimeout(200);
  }
  await page.waitForSelector('.loading', { state: 'detached', timeout: 15000 });
  await page.waitForFunction(async () => { try { return !!(await (window as any).__iron.call('schema')).types; } catch { return false; } }, undefined, { timeout: 15000 });
  await page.getByRole('tab', { name: 'Model', exact: true }).click();
  await page.evaluate(({ endpoint, token }) => (window as any).__iron.configureAgent({ provider: 'codex', endpoint, token }), { endpoint, token });
}
const model = () => page.locator('.model-view');
const prompt = () => model().getByRole('textbox', { name: 'Agent instruction' });
const chats = () => model().getByRole('button', { name: /^Chats:/ });
async function send(text: string, done: number) {
  await prompt().fill(text);
  await prompt().press('Enter');
  await page.waitForFunction(n => document.querySelectorAll('.model-view .chat-turn[data-status="completed"]').length === n, done, { timeout: 15000 });
}
async function listed(): Promise<string[]> {
  await chats().click();
  const items = await page.locator('.thread-menu [data-thread] .label').allInnerTexts();
  await page.keyboard.press('Escape');
  return items;
}

try {
  await page.goto(url);
  await ready();
  const scope = await page.evaluate(() => (window as any).__iron.scope().current.id);
  assert.match(await chats().getAttribute('aria-label') ?? '', /New chat/);
  // A first chat, then a second: New chat clears the panel.
  await send('chat-test-prose first chat', 1);
  assert.match(await chats().getAttribute('aria-label') ?? '', /chat-test-prose first chat/);
  await model().getByRole('button', { name: 'New chat', exact: true }).click();
  assert.equal(await model().locator('.chat-turn').count(), 0);
  assert.equal(await model().getByRole('button', { name: 'New chat', exact: true }).isDisabled(), true, 'an empty chat is new already');
  await send('chat-test-prose second chat', 1);
  assert.deepEqual(await listed(), ['chat-test-prose second chat', 'chat-test-prose first chat']);
  // Open the first again: its turn is back, the composer is its own.
  await chats().click();
  await page.locator('.thread-menu [data-thread]', { hasText: 'first chat' }).click();
  await page.waitForFunction(() => /first chat/.test(document.querySelector('.model-view .user-message')?.textContent ?? ''));
  assert.equal(await model().locator('.chat-turn').count(), 1);
  await prompt().fill('an unsent draft');
  await chats().click();
  await page.screenshot({ path: '.agents/chat/history-menu.png', clip: { x: 0, y: 0, width: 700, height: 420 } });
  await page.keyboard.press('Escape');
  // Saved with the scope: after a reload both chats are listed and the one
  // shown last is shown again, with its draft.
  await page.waitForTimeout(700);
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await page.reload();
  await ready();
  assert.equal(await page.evaluate(() => (window as any).__iron.scope().current.id), scope);
  await page.waitForFunction(() => document.querySelectorAll('.model-view .chat-turn').length === 1, undefined, { timeout: 10000 });
  assert.match(await chats().getAttribute('aria-label') ?? '', /first chat/);
  assert.equal(await prompt().inputValue(), 'an unsent draft');
  // Listed by last message, not by which was opened last.
  assert.deepEqual(await listed(), ['chat-test-prose second chat', 'chat-test-prose first chat']);
  // Delete asks first; Cancel keeps it, Delete chat removes it and shows the other.
  await chats().click();
  await page.getByRole('menuitem', { name: /Delete this chat/ }).click();
  await page.getByRole('button', { name: 'Cancel' }).click();
  assert.equal((await listed()).length, 2);
  await chats().click();
  await page.getByRole('menuitem', { name: /Delete this chat/ }).click();
  await page.getByRole('button', { name: 'Delete chat' }).click();
  assert.deepEqual(await listed(), ['chat-test-prose second chat']);
  await page.waitForFunction(() => /second chat/.test(document.querySelector('.model-view .user-message')?.textContent ?? ''));
  // The Lesson panel has its own history.
  await page.getByRole('tab', { name: 'Lesson', exact: true }).click();
  const lessonChats = page.locator('.narrative').getByRole('button', { name: /^Chats:/ });
  await lessonChats.waitFor();
  assert.match(await lessonChats.getAttribute('aria-label') ?? '', /New chat/);
  assert.deepEqual(errors, []);
  console.log('PASS CHAT HISTORY: new chat, past chats menu, open, draft per chat, saved with the scope across reload, delete with confirm, separate Lesson history; no page errors');
} catch (error) {
  await page.screenshot({ path: '.agents/chat/history-failure.png' }).catch(() => {});
  throw error;
} finally {
  clearTimeout(deadline);
  bridge.stop();
  await browser.close();
}
