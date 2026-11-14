// Selection-wide inspector regression. Build + serve first; DPR=1 or 2.
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
const dir = resolve(import.meta.dir, '../../../.agents/multi-properties');
mkdirSync(dir, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'] });
const dpr = Number(process.env.DPR ?? 1);
const page = await browser.newPage({ viewport: { width: 1680, height: 1000 }, deviceScaleFactor: dpr });
page.setDefaultTimeout(10000);
const errors: string[] = [];
page.on('pageerror', e => errors.push(String(e)));
const assert = (ok: unknown, why: string) => { if (!ok) throw new Error(why); };
const call = (name: string, args: unknown[]) => page.evaluate(([name, args]) => (window as any).__iron.call(name, args), [name, args] as const);
const save = () => page.evaluate(async () => JSON.parse(await (window as any).__iron.save()));
const until = async (check: () => Promise<boolean>) => { const end = Date.now() + 5000; while (Date.now() < end) { if (await check()) return; await page.waitForTimeout(40); } throw new Error('state did not settle'); };
const edit = async (xml: string) => { const r = await page.evaluate(xml => (window as any).__iron.edit(xml), xml); assert(r.ok, JSON.stringify(r)); };
const undo = async () => { await page.evaluate(() => (window as any).__iron.undo()); };
const input = (label: string) => page.getByRole('textbox', { name: label, exact: true });
const type = async (label: string, value: string) => { await input(label).fill(value); await input(label).press('Tab'); };
try {
  await page.goto(process.env.URL ?? 'http://localhost:5173/Ironfell/?gfx=webgl2&demo&layout=default');
  await page.waitForSelector('.loading', { state: 'detached', timeout: 60000 });
  await until(async () => {
    try { await call('schema', []); return (await page.evaluate(() => (window as any).__iron.view({ fidelity: 'full' }))).includes('name="controls"'); }
    catch { return false; }
  });
  const view = await page.evaluate(() => (window as any).__iron.view({ fidelity: 'full' }));
  const parent = view.match(/<composition id="([^"]+)" name="controls"/)[1];
  await edit(`<edit>
    <rect in="${parent}" id="$a" name="multi A" x="400" y="100" w="120" h="90" color="#ff0000"/><add id="$a" stroke.width="2" stroke.color="#aa0000"/>
    <rect in="${parent}" id="$b" name="multi B" x="580" y="100" w="120" h="90" color="#0000ff"/><add id="$b" stroke.width="5" stroke.color="#0000aa"/>
    <ellipse in="${parent}" id="$c" name="multi C" x="760" y="140" rx="40" ry="40" color="#ff0000"/>
    <text in="${parent}" id="$t" name="multi text A" x="400" y="300" text.content="First box" font.size="24" font.weight="400" color="#ff0000"/>
    <text in="${parent}" id="$u" name="multi text B" x="580" y="300" text.content="Second box" font.size="36" font.weight="700" color="#0000ff"/>
  </edit>`);
  const initial = await save();
  const id = (name: string): string => Object.keys(initial.facets.name).find(k => initial.facets.name[k].text === name)!;
  const [a, b, c, t, u] = ['multi A', 'multi B', 'multi C', 'multi text A', 'multi text B'].map(id);
  const select = async (nodes: string[]) => {
    const r = await call('select', [JSON.stringify(nodes.map(node => ({ node, panel: 'controls' }))), 'replace']);
    assert(r.ok, JSON.stringify(r));
    await until(async () => await page.locator('.inspector').getAttribute('data-selection') === nodes.join(' '));
    await until(async () => (await page.locator('.title-name').textContent()) === (nodes.length > 1 ? `${nodes.length} selected` : initial.facets.name[nodes[0]].text));
  };
  await page.evaluate(() => (window as any).__dock.getPanel('controls').api.setActive());
  await select([a, b]);
  await until(async () => await input('Weight').getAttribute('placeholder') === 'Mixed');
  await page.screenshot({ path: resolve(dir, `mixed-shapes-${dpr}x.png`) });
  await type('Weight', '8');
  await until(async () => { const d = await save(); return d.facets.stroke[a].width === 8 && d.facets.stroke[b].width === 8; });
  await undo();
  await until(async () => { const d = await save(); return d.facets.stroke[a].width === 2 && d.facets.stroke[b].width === 5; });
  console.log('mixed stroke width: both targets, one undo');
  const fillSection = page.locator('[data-section="fill"]');
  assert((await fillSection.innerText()).includes('Click + to replace mixed content'), 'mixed fill should offer explicit replacement');
  const paintsBelowEffects = await page.evaluate(() => !!(document.querySelector('[data-section="effects"]')!.compareDocumentPosition(document.querySelector('[data-section="selection-colors"]')!) & Node.DOCUMENT_POSITION_FOLLOWING));
  assert(paintsBelowEffects, 'selection colors should appear below effects');
  await page.locator('[data-section="selection-colors"]').scrollIntoViewIfNeeded();
  await page.screenshot({ path: resolve(dir, `paints-lower-${dpr}x.png`) });
  await fillSection.getByRole('button', { name: 'Replace mixed fill', exact: true }).click();
  await until(async () => { const d = await save(); return JSON.stringify(d.facets.fill[a]) === JSON.stringify(d.facets.fill[b]); });
  await input('Fill colour hex').fill('44AA66'); await input('Fill colour hex').press('Tab');
  await until(async () => { const d = await save(); return Math.abs(d.facets.fill[a].color.rgba[1] - 170 / 255) < .001 && JSON.stringify(d.facets.fill[a]) === JSON.stringify(d.facets.fill[b]); });
  // A shared fill's picker drag: the row must not flip to Mixed mid-scrub
  // (which closed the picker, cancelled the drag and selected the panel's text).
  await page.getByRole('button', { name: 'Fill paint settings', exact: true }).click();
  const fillSquare = page.getByRole('slider', { name: 'Saturation and brightness', exact: true });
  await fillSquare.hover();
  const fillBox = (await fillSquare.boundingBox())!;
  const textBefore = await page.evaluate(() => String(document.getSelection()));
  await page.mouse.move(fillBox.x + fillBox.width * .2, fillBox.y + fillBox.height * .3);
  await page.mouse.down(); await page.mouse.move(fillBox.x + fillBox.width * .8, fillBox.y + fillBox.height * .1, { steps: 8 });
  await page.waitForTimeout(150);
  assert(await fillSquare.isVisible(), 'shared fill picker closed mid-drag');
  await page.mouse.up();
  await until(async () => { const d = await save(); return Math.abs(d.facets.fill[a].color.rgba[1] - 170 / 255) > .01 && JSON.stringify(d.facets.fill[a]) === JSON.stringify(d.facets.fill[b]); });
  assert(await fillSquare.isVisible(), 'shared fill picker closed after the drag');
  assert(await page.evaluate(() => String(document.getSelection())) === textBefore, 'picker drag selected page text');
  await page.getByRole('button', { name: 'Close fill settings', exact: true }).click();
  await undo();
  await until(async () => await input('Fill colour hex').inputValue() === '44AA66');
  console.log('shared fill: picker drag recolours both and stays open, one undo');
  await undo();
  await until(async () => await input('Fill colour hex').inputValue() === '808080');
  await undo();
  await until(async () => (await fillSection.innerText()).includes('Click + to replace mixed content'));
  console.log('mixed fills: + replaces all, one undo restores original paints; selection colors below effects');
  // The first selected object stays fixed while the rest of the selection changes.
  await select([a, c]);
  await until(async () => await input('Weight').getAttribute('placeholder') === 'Mixed');
  await type('Weight', '6');
  await until(async () => { const d = await save(); return d.facets.stroke[a].width === 6 && d.facets.stroke[c]?.width === 6 && d.facets.stroke[b].width === 5; });
  await undo();
  await until(async () => !(await save()).facets.stroke[c]);
  console.log('heterogeneous selection: optional facet added, one undo restores absence');
  await select([a, b]);
  assert(await input('Stroke opacity').inputValue() === '100', 'equal alpha should not show Mixed for differing RGB');
  await type('Stroke opacity', '50');
  await until(async () => { const d = await save(); return [a, b].every(id => Math.abs(d.facets.stroke[id].color.rgba[3] - .5) < .01); });
  let d = await save();
  assert(d.facets.stroke[a].color.rgba[0] > .6 && d.facets.stroke[b].color.rgba[2] > .6, 'opacity replaced original RGB');
  await undo();
  await until(async () => (await save()).facets.stroke[a].color.rgba[3] === 1);
  const width = input('Weight'); await width.scrollIntoViewIfNeeded();
  let box = (await width.boundingBox())!;
  await page.keyboard.down('Alt'); await page.mouse.move(box.x + 20, box.y + 12); await page.mouse.down(); await page.mouse.move(box.x + 40, box.y + 12, { steps: 5 });
  await page.waitForTimeout(200);
  assert((await save()).facets.stroke[a].width === 2, 'preview leaked into document');
  await page.mouse.up(); await page.keyboard.up('Alt');
  await until(async () => { const d = await save(); return d.facets.stroke[a].width === 22 && d.facets.stroke[b].width === 22; });
  await undo(); await until(async () => (await save()).facets.stroke[b].width === 5);
  console.log('opacity preserves RGB; numeric scrub changes both in one undo');
  await page.getByRole('button', { name: 'Stroke paint settings', exact: true }).click();
  const square = page.getByRole('slider', { name: 'Saturation and brightness', exact: true });
  await square.hover();
  const colorBox = (await square.boundingBox())!;
  await page.mouse.move(colorBox.x + colorBox.width * .3, colorBox.y + colorBox.height * .4);
  await page.mouse.down(); await page.mouse.move(colorBox.x + colorBox.width * .7, colorBox.y + colorBox.height * .2, { steps: 5 });
  await page.mouse.up();
  await until(async () => { const d = await save(); return JSON.stringify(d.facets.stroke[a].color) === JSON.stringify(d.facets.stroke[b].color); });
  await page.getByRole('button', { name: 'Close stroke settings', exact: true }).click();
  await undo();
  await until(async () => { const d = await save(); return JSON.stringify(d.facets.stroke[a].color) === JSON.stringify(initial.facets.stroke[a].color) && JSON.stringify(d.facets.stroke[b].color) === JSON.stringify(initial.facets.stroke[b].color); });
  console.log('color picker previews and commits both paints in one undo');
  // Original selection colors edit only matching paints.
  await select([a, c]);
  const selectedColors = page.locator('[data-section="selection-colors"]');
  const fill = selectedColors.getByRole('textbox', { name: 'Fill #FF0000 colour hex', exact: true });
  await fill.fill('00FF00'); await fill.press('Tab');
  await until(async () => { const d = await save(); return d.facets.fill[a].color.rgba[1] === 1 && d.facets.fill[c].color.rgba[1] === 1 && d.facets.fill[b].color.rgba[2] === 1; });
  await undo(); await until(async () => (await save()).facets.fill[a].color.rgba[0] === 1);
  console.log('selection colors replace matching paints only');
  // Validation happens before applying any member of a batch.
  const before = await save();
  const rejected = await call('inspect_patch', [a, JSON.stringify({ nodes: [a, '#zzzzzz'], values: { 'stroke.width': '9' } })]);
  assert(!rejected.ok && JSON.stringify(await save()) === JSON.stringify(before), 'invalid multi-edit partially applied');
  await edit(`<edit><set id="${a}" stroke.width="{= ${b}.stroke.width * 2}"/></edit>`);
  const linkedBefore = JSON.stringify(await save());
  const conflict = await call('inspect_patch', [a, JSON.stringify({ nodes: [a, b], values: { 'stroke.width': '8' } })]);
  assert(!conflict.ok && JSON.stringify(await save()) === linkedBefore, 'conflicting source writes partially applied');
  const options = JSON.stringify({ nodes: [a, b] });
  const gesture = await call('inspect_scrub', [a, 'stroke.width', 'begin', 0, 0, options]);
  assert(gesture.ok, 'bound scrub did not begin');
  const scrubConflict = await call('inspect_scrub', [a, 'stroke.width', 'move', gesture.token, 8, options]);
  assert(!scrubConflict.ok && JSON.stringify(await save()) === linkedBefore, 'conflicting scrub changed the document');
  await undo();
  await until(async () => (await save()).facets.stroke[a].width === 2);
  console.log('invalid targets and conflicting shared bindings reject atomically');
  await select([t, u]);
  await until(async () => await input('Font size').getAttribute('placeholder') === 'Mixed');
  assert((await page.getByRole('button', { name: 'Font weight', exact: true }).textContent())?.trim() === 'Mixed', 'weight copied first value');
  await page.screenshot({ path: resolve(dir, `mixed-text-${dpr}x.png`) });
  await page.locator('[data-section="text"]').getByRole('button', { name: 'Bold', exact: true }).click();
  await until(async () => { const d = await save(); return d.facets.font[t].weight === 700 && d.facets.font[u].weight === 700; });
  await undo(); await until(async () => (await save()).facets.font[t].weight === 400);
  await page.locator('[data-section="text"]').getByRole('button', { name: 'Italic', exact: true }).click();
  await until(async () => { const d = await save(); return d.facets.font[t].italic && d.facets.font[u].italic; });
  await type('Font size', '48');
  await until(async () => { const d = await save(); return d.facets.font[t].size === 48 && d.facets.font[u].size === 48; });
  await undo(); await until(async () => { const d = await save(); return d.facets.font[t].size === 24 && d.facets.font[u].size === 36; });
  await type('Text colour', '00ff00');
  await until(async () => { const d = await save(); return d.facets.fill[t].color.rgba[1] === 1 && d.facets.fill[u].color.rgba[1] === 1; });
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.screenshot({ path: resolve(dir, `text-dark-${dpr}x.png`) });
  console.log('text: mixed values, Bold, Italic, size, color and one-step undo');
  assert(!errors.length, errors.join('\n'));
  console.log(`multi-properties DPR=${dpr}: pass (no page errors)`);
} catch (error) {
  console.log(await page.locator('.inspector').innerText());
  const d = await save();
  console.log(Object.keys(d.facets.name).filter(id => d.facets.name[id].text.startsWith('multi ')).map(id => [id, d.facets.name[id].text, d.facets.stroke?.[id]]));
  await page.screenshot({ path: resolve(dir, `failure-${dpr}x.png`) });
  throw error;
} finally { await browser.close(); }
