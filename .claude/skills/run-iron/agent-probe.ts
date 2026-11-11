/**
 * Agent probe: send prompts to Iron's in-app agent (Model chat) and report what
 * it did. Starts its own Vite and agent bridge on free ports (scripts/dev-agent.ts),
 * drives headless Chrome, and writes .agents/probe/<timestamp>/ with report.md,
 * the document before and after each turn, a screenshot per turn and the run's
 * agent log. See SKILL.md, "Agent probe".
 *
 *   bun .claude/skills/run-iron/agent-probe.ts "Draw a pendulum" "Now label it"
 *   bun .claude/skills/run-iron/agent-probe.ts --file prompts.txt   # prompts split by lines of ---
 *
 * PROVIDER=codex|claude (default codex)   MODEL=<id> (default: the runtime's)
 * FIXTURE=1   the scripted runtime (tests/fixtures/fake-agent-runtime.ts), no model usage
 * TURN_MINUTES=10   per-turn limit; the turn is stopped and the run ends there
 * APP_PATH='/Ironfell/?gfx=webgl2&demo'   page path and query on the probe's own server
 * RUNTIME=worker|main   HEADED=1 to watch   SIZE=1600x1000
 *
 * A prompt line `@image <path>` attaches that image (PNG, JPEG, SVG, HEIC…) to the
 * turn through the composer's file input, as a dropped image would be; the line
 * itself is not sent.
 */
import { chromium } from 'playwright-core';
import { createServer } from 'node:net';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, relative, resolve } from 'node:path';
import { startAgentDev } from '../../../scripts/dev-agent';
import { codexRuntime } from '../../../scripts/agent-bridge/runtimes';

const root = fileURLToPath(new URL('../../..', import.meta.url));
const args = process.argv.slice(2);
if (args.includes('--help') || args.length === 0) {
  console.log(readFileHeader());
  process.exit(args.length === 0 ? 1 : 0);
}
const prompts: string[] = [];
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--file') {
    const text = await readFile(resolve(args[++i] ?? ''), 'utf8');
    prompts.push(...text.split(/^---\s*$/m).map(p => p.trim()).filter(Boolean));
  } else prompts.push(args[i]);
}
if (!prompts.length) throw new Error('No prompts given');

const provider = (process.env.PROVIDER ?? 'codex') as 'codex' | 'claude';
if (!['codex', 'claude'].includes(provider)) throw new Error('PROVIDER must be codex or claude');
const fixture = process.env.FIXTURE === '1';
const model = process.env.MODEL || undefined;
const turnMs = Number(process.env.TURN_MINUTES ?? 10) * 60_000;
const appPath = process.env.APP_PATH ?? '/Ironfell/?gfx=webgl2&demo';
const runtimeMode = process.env.RUNTIME ?? 'worker';
const [width, height] = (process.env.SIZE ?? '1600x1000').split('x').map(Number);

const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
const runDir = join(root, '.agents', 'probe', stamp);
const logDir = join(runDir, 'log');
await mkdir(runDir, { recursive: true });

function readFileHeader() {
  const source = spawnSync('sed', ['-n', '2,16p', fileURLToPath(import.meta.url)], { encoding: 'utf8' }).stdout;
  return source.replace(/^ \*\/?\s?/gm, '');
}

async function freePort(): Promise<number> {
  return new Promise((ok, fail) => {
    const server = createServer().listen(0, '127.0.0.1', () => {
      const port = (server.address() as { port: number }).port;
      server.close(() => ok(port));
    }).on('error', fail);
  });
}

const fake = fixture ? codexRuntime(resolve(root, 'tests/fixtures/fake-agent-runtime.ts')) : undefined;
const token = randomBytes(24).toString('hex');
const session = await startAgentDev({
  root, token, quiet: true, appPort: await freePort(), bridgePort: 0, logDir,
  totalTimeoutMs: turnMs + 60_000,
  ...(fake ? { runtimes: { codex: fake, claude: fake } } : {}),
});
const url = new URL(appPath, session.url).href;

interface TurnRow { index: number; prompt: string; id: string | null; status: string; ms: number; shown: string; screenshot: string; before: string; after: string }
const turns: TurnRow[] = [];
const pageErrors: string[] = [];
const consoleErrors: string[] = [];
const browser = await chromium.launch({
  executablePath: process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: process.env.HEADED !== '1', args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--no-sandbox'],
});
let fatal = '';
try {
  const page = await browser.newPage({ viewport: { width, height } });
  page.setDefaultTimeout(15000);
  page.on('pageerror', error => pageErrors.push(String(error)));
  // The page's boot-time pairing tries the default bridge on 4318 and is refused
  // when another bridge (or none) is there; the probe pairs with its own instead.
  let pairing = false;
  page.on('console', message => {
    if (message.type() !== 'error') return;
    const text = message.text();
    if (text.includes(':4318/pair')) { pairing = true; return; }
    if (pairing && text.startsWith('Failed to load resource: net::ERR_')) { pairing = false; return; }
    consoleErrors.push(text);
  });
  await page.goto(url);
  await page.evaluate(mode => {
    const key = 'iron.settings.v1';
    localStorage.setItem(key, JSON.stringify({ ...JSON.parse(localStorage.getItem(key) ?? '{}'), runtime: mode }));
  }, runtimeMode);
  await page.reload();
  await page.waitForFunction(() => !!(window as any).__iron, undefined, { timeout: 30000 });
  await page.waitForSelector('.loading', { state: 'detached', timeout: 30000 });
  await page.waitForFunction(async () => {
    try { return !!(await (window as any).__iron.call('schema')).types; } catch { return false; }
  }, undefined, { timeout: 30000 });
  // The left side's view strip (its tab bar is hidden) switches to the Model chat.
  await page.getByRole('button', { name: 'Model (the agent)', exact: true }).first().click();
  // Automatic pairing looks on port 4318; the probe's bridge is on its own port.
  await page.evaluate(({ endpoint, token, provider, model }) =>
    (window as any).__iron.configureAgent({ provider, endpoint, token, ...(model ? { model } : {}) }),
  { endpoint: session.endpoint, token, provider, model });
  await page.waitForFunction(() => (window as any).__iron.agentState().canAsk, undefined, { timeout: 20000 });
  const prompt = page.getByRole('textbox', { name: 'Agent instruction' });
  const read = () => page.evaluate(() => (window as any).__iron.view({ fidelity: 'full' }) as Promise<string>);
  // The document once it has stopped changing (fonts loading reflow text and
  // tables after boot): the same reading three times half a second apart, or 15 s.
  const view = async () => {
    let last = await read(), same = 0;
    for (const end = Date.now() + 15000; same < 2 && Date.now() < end;) {
      await page.waitForTimeout(500);
      const next = await read();
      same = next === last ? same + 1 : 0;
      last = next;
    }
    return last;
  };

  for (const [i, text] of prompts.entries()) {
    const n = i + 1;
    const before = `turn-${n}-before.xml`, after = `turn-${n}-after.xml`, shot = `turn-${n}.png`;
    await writeFile(join(runDir, before), await view());
    const seen = await page.locator('.chat-turn').evaluateAll(ts => ts.map(t => t.getAttribute('data-turn-id')));
    const images = [...text.matchAll(/^@image\s+(.+)$/gm)].map(m => resolve(m[1].trim()));
    const message = text.replace(/^@image\s+.+\n?/gm, '').trim();
    if (images.length) {
      await page.locator('input[type=file][accept^="image/png"]').setInputFiles(images);
      await page.waitForFunction(n => document.querySelectorAll('.attachment img').length >= n, images.length, { timeout: 30000 });
    }
    await prompt.fill(message);
    await prompt.press('Enter');
    const started = Date.now();
    console.log(`turn ${n}/${prompts.length}: sent (limit ${turnMs / 60000} min)`);
    // The turn this prompt started: the first turn id the page had not shown before.
    const ended = (timeout: number) => page.waitForFunction(seen => [...document.querySelectorAll('.chat-turn')]
      .some(t => !seen.includes(t.getAttribute('data-turn-id')) && t.getAttribute('data-status') !== 'running'), seen, { timeout, polling: 500 });
    let timedOut = false;
    await ended(turnMs).catch(async () => {
      timedOut = true;
      await page.getByRole('button', { name: 'Stop the agent' }).click().catch(() => {});
      await ended(15000).catch(() => {});
    });
    const last = page.locator('.chat-turn').last();
    const fresh = await page.locator('.chat-turn').evaluateAll((ts, seen) => ts.map(t => t.getAttribute('data-turn-id')).filter(id => !seen.includes(id)), seen);
    const turn = fresh.length ? page.locator(`.chat-turn[data-turn-id="${fresh[0]}"]`) : last;
    const id = await turn.getAttribute('data-turn-id');
    const state = await turn.getAttribute('data-status');
    const status = timedOut ? `timed out (${state})` : state ?? 'unknown';
    await writeFile(join(runDir, after), await view());
    await page.screenshot({ path: join(runDir, shot) });
    turns.push({ index: n, prompt: text, id, status, ms: Date.now() - started, shown: await turn.innerText(), screenshot: shot, before, after });
    console.log(`turn ${n}/${prompts.length}: ${status} in ${((Date.now() - started) / 1000).toFixed(1)} s`);
    if (timedOut) break;
  }
} catch (error) {
  fatal = error instanceof Error ? error.stack ?? error.message : String(error);
  console.error(`Probe stopped: ${fatal}`);
} finally {
  await new Promise(r => setTimeout(r, 400)); // the page posts each turn's outcome without waiting
  await browser.close();
  await session.stop();
}

// The run's agent log: one file a day, normally one (beside it, images/ holds
// the turns' pictures).
const lines = (await Promise.all((await readdir(logDir).catch(() => [] as string[])).filter(f => f.endsWith('.jsonl')).sort().map(f => readFile(join(logDir, f), 'utf8')))).join('');
await writeFile(join(runDir, 'agent-log.jsonl'), lines);
const records = lines.trim() ? lines.trim().split('\n').map(line => JSON.parse(line)) : [];

const fence = (text: string, lang = '') => { const ticks = text.includes('```') ? '~~~~' : '```'; return `${ticks}${lang}\n${text.trimEnd()}\n${ticks}`; };
const clip = (text: string, max: number) => text.length > max ? `${text.slice(0, max)}\n… (${text.length - max} more characters; see agent-log.jsonl)` : text;
const out: string[] = [
  `# Agent probe ${stamp}`, '',
  `- Provider: ${fixture ? `${provider} (fixture runtime)` : provider}${model ? `, model ${model}` : ''}`,
  `- Page: ${url} (${runtimeMode})`,
  `- Turns: ${turns.length} of ${prompts.length} sent${fatal ? ' (the probe stopped early)' : ''}`,
  `- Page errors: ${pageErrors.length}, console errors: ${consoleErrors.length}`, '',
];
if (fatal) out.push('## Probe failure', '', fence(fatal), '');
for (const turn of turns) {
  const outcome = records.find(r => r.kind === 'turn' && r.turn === turn.id);
  const calls = records.filter(r => r.kind === 'call' && r.turn === turn.id);
  out.push(`## Turn ${turn.index}: ${turn.status}, ${(turn.ms / 1000).toFixed(1)} s`, '', '**Prompt**', '', fence(turn.prompt), '');
  if (outcome) {
    out.push(`- ok: ${outcome.ok}, attempts: ${outcome.attempts}, model calls: ${calls.length}${outcome.error ? `, error: ${outcome.error}` : ''}`);
    if (outcome.outcome) out.push(`- applied: ${outcome.outcome.ok ? 'yes' : 'no'}${outcome.outcome.summary ? ` — ${outcome.outcome.summary}` : ''}${outcome.outcome.failure ? `; failure: ${outcome.outcome.failure}` : ''}`);
    for (const step of outcome.trace ?? []) out.push(`- step ${step.attempt}: ${step.kind}${'ok' in step ? (step.ok ? ' ok' : ' rejected') : ''}${step.failure ? ` — ${step.failure}` : ''}${step.summary ? ` — ${step.summary}` : ''}${step.message ? ` — ${step.message}` : ''}${step.ids ? ` ${step.ids.join(', ')}` : ''}`);
    out.push('');
    if (outcome.note) out.push('**Reply**', '', clip(outcome.note, 6000), '');
    if (outcome.edit) out.push('**Edit**', '', fence(clip(outcome.edit, 8000), 'xml'), '');
  } else {
    out.push('_No outcome in the agent log; what the chat showed:_', '', fence(clip(turn.shown, 4000)), '');
  }
  if (calls.length > 1 || (outcome && !outcome.ok)) {
    out.push('<details><summary>Model replies, call by call</summary>', '');
    for (const [i, call] of calls.entries()) out.push(`Call ${i + 1} (${call.model ?? 'default model'}, ${(call.ms / 1000).toFixed(1)} s${call.error ? `, error: ${call.error}` : ''})`, '', fence(clip(call.reply ?? '', 4000)), '');
    out.push('</details>', '');
  }
  const diff = spawnSync('diff', ['-u', turn.before, turn.after], { cwd: runDir, encoding: 'utf8' }).stdout;
  out.push(`**Document change** (${turn.before} → ${turn.after})`, '', diff ? fence(clip(diff, 8000), 'diff') : '_No change._', '', `![turn ${turn.index}](${turn.screenshot})`, '');
}
if (pageErrors.length) out.push('## Page errors', '', fence(pageErrors.join('\n')), '');
if (consoleErrors.length) out.push('## Console errors', '', fence(clip(consoleErrors.join('\n'), 8000)), '');
const report = join(runDir, 'report.md');
await writeFile(report, out.join('\n'));
const failed = !!fatal || turns.some(t => t.status !== 'completed');
console.log(`${failed ? 'PROBE DONE with failures' : 'PROBE DONE'}: ${turns.length} turn(s), ${pageErrors.length} page error(s). Report: ${relative(process.cwd(), report) || report}`);
process.exit(fatal ? 1 : 0);
