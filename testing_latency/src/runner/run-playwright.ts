import { chromium, type Browser, type Page } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { pointAt } from '../shared/paths';
import {
  APP_IDS,
  DEFAULT_TRIAL,
  PATH_IDS,
  type TestAppId,
  type TrialConfig,
  type TrialPathId,
  type TrialResult,
} from '../shared/protocol';

const __dirname = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(__dirname, '../..');
const defaultBaseUrl = 'http://127.0.0.1:5177';

interface RunnerOptions {
  apps: TestAppId[];
  paths: TrialPathId[];
  baseUrl: string;
  headed: boolean;
  keepServer: boolean;
  outputDir: string;
}

async function main() {
  const options = parseArgs();
  let server: ChildProcess | null = null;

  if (!process.env.LATENCY_TEST_BASE_URL) {
    server = spawn('npx', ['vite', '--host', '127.0.0.1', '--port', '5177', '--strictPort'], {
      cwd: projectRoot,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: process.env,
    });
    server.stdout?.on('data', (chunk) => process.stdout.write(chunk));
    server.stderr?.on('data', (chunk) => process.stderr.write(chunk));
    await waitForServer(options.baseUrl);
  }

  let browser: Browser | null = null;
  try {
    browser = await chromium.launch({
      headless: !options.headed,
      args: ['--enable-unsafe-webgpu'],
    });
    await mkdir(options.outputDir, { recursive: true });
    for (const app of options.apps) {
      for (const path of options.paths) {
        const result = await runOne(browser, options.baseUrl, app, path);
        const fileName = `${safeStamp(result.startedAtIso)}_${app}_${path}.json`;
        const filePath = resolve(options.outputDir, fileName);
        await writeFile(filePath, JSON.stringify(result, null, 2));
        printSummary(result, filePath);
      }
    }
  } finally {
    await browser?.close();
    if (server && !options.keepServer) {
      server.kill('SIGTERM');
    }
  }
}

async function runOne(
  browser: Browser,
  baseUrl: string,
  app: TestAppId,
  path: TrialPathId,
): Promise<TrialResult> {
  const page = await browser.newPage({
    viewport: { width: 1220, height: 760 },
    deviceScaleFactor: 1,
  });
  page.on('console', (message) => {
    if (message.type() === 'error') {
      console.error(`[${app}] browser console error: ${message.text()}`);
    }
  });

  try {
    await page.goto(`${baseUrl}/test/${app}`, { waitUntil: 'networkidle' });
    await page.waitForFunction(() => Boolean(window.__latency?.ready), undefined, { timeout: 15_000 });

    const trial: TrialConfig = {
      ...DEFAULT_TRIAL,
      app,
      path,
      referenceMode: 'scripted',
    };
    await page.evaluate((config) => window.__latency!.startTrial(config), trial);

    const canvasBox = await page.locator('canvas').boundingBox();
    if (!canvasBox) throw new Error(`No canvas box found for ${app}`);

    const start = pointAt(trial, 0);
    await page.mouse.move(canvasBox.x + start.x, canvasBox.y + start.y);
    await page.mouse.down();
    await page.evaluate(() => window.__latency!.beginMotion());

    const started = performance.now();
    const intervalMs = 1000 / trial.sampleHz;
    for (;;) {
      const elapsed = performance.now() - started;
      if (elapsed >= trial.durationMs) break;
      const point = pointAt(trial, elapsed);
      await page.mouse.move(canvasBox.x + point.x, canvasBox.y + point.y);
      await sleep(intervalMs);
    }

    const end = pointAt(trial, trial.durationMs);
    await page.mouse.move(canvasBox.x + end.x, canvasBox.y + end.y);
    await page.mouse.up();
    await sleep(160);

    return await page.evaluate(() => window.__latency!.stopTrial());
  } finally {
    await page.close();
  }
}

function parseArgs(): RunnerOptions {
  const args = new Map<string, string | boolean>();
  for (const raw of process.argv.slice(2)) {
    if (raw.startsWith('--') && raw.includes('=')) {
      const [key, value] = raw.slice(2).split(/=(.*)/s, 2);
      args.set(key, value);
    } else if (raw.startsWith('--')) {
      args.set(raw.slice(2), true);
    }
  }

  return {
    apps: parseList(args.get('apps'), APP_IDS),
    paths: parseList(args.get('paths'), PATH_IDS),
    baseUrl: String(args.get('base-url') || process.env.LATENCY_TEST_BASE_URL || defaultBaseUrl),
    headed: Boolean(args.get('headed')),
    keepServer: Boolean(args.get('keep-server')),
    outputDir: resolve(projectRoot, String(args.get('out') || 'results')),
  };
}

function parseList<T extends string>(value: string | boolean | undefined, allowed: readonly T[]): T[] {
  if (!value || value === true) return [...allowed];
  const requested = value.split(',').map((item) => item.trim()).filter(Boolean);
  const invalid = requested.filter((item) => !allowed.includes(item as T));
  if (invalid.length > 0) {
    throw new Error(`Invalid value(s): ${invalid.join(', ')}. Allowed: ${allowed.join(', ')}`);
  }
  return requested as T[];
}

async function waitForServer(baseUrl: string) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(baseUrl);
      if (response.ok) return;
    } catch {
      await sleep(250);
    }
  }
  throw new Error(`Timed out waiting for ${baseUrl}`);
}

function printSummary(result: TrialResult, filePath: string) {
  const p50 = fmt(result.summary.p50LatencyMs);
  const p95 = fmt(result.summary.p95LatencyMs);
  const max = fmt(result.summary.maxLatencyMs);
  console.log(`${result.app} / ${result.path}: p50=${p50}ms p95=${p95}ms max=${max}ms -> ${filePath}`);
}

function fmt(value: number | null) {
  return value === null ? 'n/a' : value.toFixed(2);
}

function safeStamp(iso: string) {
  return iso.replaceAll(':', '-').replaceAll('.', '-');
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
