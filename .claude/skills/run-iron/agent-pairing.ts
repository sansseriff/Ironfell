import { chromium } from 'playwright-core';
import { once } from 'node:events';
import { randomBytes } from 'node:crypto';
import assert from 'node:assert/strict';
import { createAgentBridge } from '../../../scripts/agent-bridge/server';

const url = process.env.URL ?? 'http://127.0.0.1:5175/Ironfell/?gfx=webgl2&demo';
if (!new URL(url).searchParams.has('demo')) throw new Error('Pairing tests require an ephemeral ?demo URL');
let token = randomBytes(24).toString('hex');
let calls = 0;
function makeBridge() {
  const runtime = async () => { calls++; return { text: 'Automatically paired, without document edits.' }; };
  return createAgentBridge({ token, origins: [new URL(url).origin], allowPairing: true, runtimes: { codex: runtime, claude: runtime } });
}
let bridge = makeBridge();
bridge.server.listen(0, '127.0.0.1');
await once(bridge.server, 'listening');
const port = (bridge.server.address() as { port: number }).port;
const endpoint = `http://127.0.0.1:${port}`;
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
const deadline = setTimeout(() => { bridge.stop(); void browser.close(); }, 55000);
const page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
page.setDefaultTimeout(8000);
const errors: string[] = [];
page.on('pageerror', error => errors.push(String(error)));
async function ready() {
  await page.getByRole('tab', { name: 'Model', exact: true }).waitFor({ timeout: 15000 });
  await page.waitForSelector('.loading', { state: 'detached', timeout: 15000 });
  await page.waitForFunction(async () => {
    try { return !!(await (window as any).__iron.call('schema')).types; } catch { return false; }
  });
  await page.getByRole('tab', { name: 'Model', exact: true }).click();
}
async function checkToken(expected: string) {
  await page.getByRole('button', { name: 'Agent settings', exact: true }).click();
  await page.waitForFunction(value => (document.querySelector('[aria-label="Bridge session token"]') as HTMLInputElement)?.value === value, expected);
  await page.getByText('Connected automatically on this computer.', { exact: false }).waitFor();
  await page.keyboard.press('Escape');
  assert.equal(page.url().includes(expected), false);
  assert.equal(await page.evaluate(value => [JSON.stringify(localStorage), JSON.stringify(sessionStorage)].some(storage => storage.includes(value)), expected), false);
}
try {
  await page.goto(url);
  await ready();
  await page.evaluate(endpoint => (window as any).__iron.configureAgent({ provider: 'codex', endpoint }), endpoint);
  await checkToken(token);
  await page.reload();
  await ready();
  await page.evaluate(endpoint => (window as any).__iron.configureAgent({ provider: 'codex', endpoint }), endpoint);
  await checkToken(token);
  const closed = once(bridge.server, 'close');
  bridge.stop();
  await closed;
  token = randomBytes(24).toString('hex');
  bridge = makeBridge();
  bridge.server.listen(port, '127.0.0.1');
  await once(bridge.server, 'listening');
  await page.getByRole('textbox', { name: 'Agent instruction' }).fill('Confirm the local connection without edits.');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  await page.locator('.chat-turn[data-status="completed"]').waitFor();
  assert.equal(calls, 1);
  await checkToken(token);
  assert.deepEqual(errors, []);
  console.log('PASS LOCAL PAIRING: autofill, reload, token rotation, authenticated send, no credential storage; no page errors');
} finally {
  clearTimeout(deadline);
  bridge.stop();
  await browser.close();
}
