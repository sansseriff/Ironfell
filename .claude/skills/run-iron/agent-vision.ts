import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdir, writeFile } from 'node:fs/promises';
import { createAgentBridge } from '../../../scripts/agent-bridge/server';

const base = process.env.URL ?? 'http://localhost:5183/Ironfell/';
const backend = process.env.BACKEND ?? 'webgpu';
const mode = process.env.RUNTIME ?? 'worker';
const dpr = Number(process.env.DPR ?? 1);
let received = 0;
const runtime = async (request: any) => {
  const images = request.messages.flatMap((m: any) => m.images ?? []);
  assert.ok(images.length > 0, 'model must receive pixels');
  assert.ok(images.every((i: any) => i.mediaType === 'image/png' && i.data.length > 50));
  received++;
  return { text: 'I can inspect the attached reference.' };
};
const token = 'vision-test-token-not-a-provider-key-12345';
const bridge = createAgentBridge({ token, origins: [new URL(base).origin], runtimes: { codex: runtime, claude: runtime } });
bridge.server.listen(0, '127.0.0.1');
await once(bridge.server, 'listening');
const endpoint = `http://127.0.0.1:${(bridge.server.address() as any).port}`;
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
const deadline = setTimeout(() => { bridge.stop(); void browser.close(); }, 55000);
try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 1000 }, deviceScaleFactor: dpr });
  const errors: string[] = [];
  page.on('pageerror', e => errors.push(String(e)));
  await page.addInitScript(mode => localStorage.setItem('iron.settings.v1', JSON.stringify({ runtime: mode })), mode);
  await page.goto(`${base}?gfx=${backend}&demo&showcase=0`);
  await page.waitForFunction(() => !!(window as any).__iron, undefined, { timeout: 30000 });
  await page.waitForSelector('.loading', { state: 'detached', timeout: 30000 });

  const png = await page.evaluate(async () => {
    const api = (window as any).__iron;
    const edit = await api.edit('<edit><composition id="$c" name="vision-test" notation="free" dims="2d"/><rect in="$c" x="0" y="0" w="100" h="60" color="#ff0000"/></edit>');
    if (!edit.ok) throw new Error(JSON.stringify(edit));
    const before = await api.save();
    const invalid = await api.call('picture', ['not-a-composition']);
    if (invalid.ok) throw new Error('Invalid composition accepted');
    const cancelled = await api.call('picture', ['vision-test']);
    await api.call('picture_poll', [cancelled.ticket, true]);
    if ((await api.call('picture_poll', [cancelled.ticket, false])).ok) throw new Error('Cancelled capture survived');
    const start = await api.call('picture', ['vision-test']);
    if (!start.ok) throw new Error(JSON.stringify(start));
    for (let n = 0; n < 300; n++) {
      const result = await api.call('picture_poll', [start.ticket, false]);
      if (!result.pending) {
        if (!result.ok) throw new Error(JSON.stringify(result));
        if (before !== await api.save()) throw new Error('Capture mutated the document');
        const image = new Image(); image.src = `data:image/png;base64,${result.image.data}`; await image.decode();
        const canvas = document.createElement('canvas'); canvas.width = image.width; canvas.height = image.height;
        const ctx = canvas.getContext('2d')!; ctx.drawImage(image, 0, 0);
        const p = ctx.getImageData(Math.floor(image.width / 2), Math.floor(image.height / 2), 1, 1).data;
        if (p[0] < 240 || p[1] > 15 || p[2] > 15 || p[3] !== 255) throw new Error(`wrong captured pixel: ${[...p]}`);
        return result.image.data;
      }
      await new Promise(r => setTimeout(r, 50));
    }
    throw new Error('Capture timed out');
  });
  await mkdir('.agents/vision', { recursive: true });
  await writeFile(`.agents/vision/capture-${backend}-${mode}-${dpr}.png`, Buffer.from(png, 'base64'));
  await page.getByRole('button', { name: 'Model (the agent)', exact: true }).first().click();
  await page.evaluate(({ endpoint, token }) => (window as any).__iron.configureAgent({ provider: 'codex', endpoint, token }), { endpoint, token });
  await page.locator('input[type=file][accept^="image/png"]').setInputFiles({ name: 'reference.png', mimeType: 'image/png', buffer: Buffer.from(png, 'base64') });
  await page.locator('.attachment img').waitFor();
  await page.getByRole('button', { name: 'Remove reference.png', exact: true }).click();
  assert.equal(await page.locator('.attachment img').count(), 0);
  await page.getByRole('textbox', { name: 'Agent instruction' }).evaluate((field, png) => {
    const data = new DataTransfer();
    data.items.add(new File([Uint8Array.from(atob(png), c => c.charCodeAt(0))], 'pasted.png', { type: 'image/png' }));
    field.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data }));
  }, png);
  await page.locator('.attachment img').waitFor();
  await page.locator('[data-chat-panel]').evaluate((form, png) => {
    const data = new DataTransfer();
    data.items.add(new File([Uint8Array.from(atob(png), c => c.charCodeAt(0))], 'dropped.png', { type: 'image/png' }));
    form.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: data }));
  }, png);
  await page.waitForFunction(() => document.querySelectorAll('.attachment img').length === 2);
  await page.locator('input[type=file][accept^="image/png"]').setInputFiles({ name: 'bad.svg', mimeType: 'image/svg+xml', buffer: Buffer.from('<svg/>') });
  await page.getByRole('alert').waitFor();
  assert.equal(await page.locator('.attachment img').count(), 2);
  await page.getByRole('textbox', { name: 'Agent instruction' }).fill('Describe this reference');
  await page.getByRole('button', { name: 'Send message', exact: true }).click();
  await page.locator('.chat-turn:not([data-status=running])').waitFor();
  assert.equal(await page.locator('.chat-turn').getAttribute('data-status'), 'completed', await page.locator('.chat-turn').innerText());
  assert.equal(received, 1);
  assert.equal(await page.locator('.user-message img').count(), 2);
  assert.equal(await page.locator('.attachment img').count(), 0);
  await page.screenshot({ path: `.agents/vision/chat-${backend}-${mode}-${dpr}.png` });
  assert.deepEqual(errors, []);
  console.log(`PASS ${backend} ${mode} DPR ${dpr}: offscreen GPU pixels, read-only capture, image attachment through bridge, transcript thumbnail`);
} finally { clearTimeout(deadline); await browser.close(); bridge.stop(); }
